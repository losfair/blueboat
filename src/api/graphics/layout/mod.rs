use std::{collections::HashMap, mem::ManuallyDrop};

use crate::{
  api::util::{mk_v8_string, v8_deserialize, v8_serialize},
  exec::Executor,
};
use anyhow::Result;
use fraction::GenericFraction;
use itertools::Itertools;
use rusty_v8 as v8;
use serde::{Deserialize, Serialize};
use std::sync::mpsc;
use thiserror::Error;
use z3::ast::Ast;

#[derive(Deserialize)]
pub struct LayoutRequest<'s> {
  boxes: Vec<serde_v8::Value<'s>>,
  metrics: Vec<Real>,
  constraints: Vec<LayoutConstraint>,
}

#[derive(Serialize)]
pub struct LayoutSolution<'s> {
  boxes: Vec<LayoutBoxSolution<'s>>,
  sat: Vec<bool>,
}

#[derive(Serialize)]
pub struct LayoutBoxSolution<'s> {
  id: serde_v8::Value<'s>,
  left: f64,
  top: f64,
  bottom: f64,
  right: f64,
  z: f64,
}

#[derive(Deserialize, Copy, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LayoutBoxProperty {
  Left,
  Top,
  Bottom,
  Right,
  Z,
}

#[derive(Deserialize)]
pub struct LayoutConstraint {
  prop: Prop,
  weight: u32,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Real {
  BoxProperty {
    /// Pass an index into `LayoutRequest.boxes` instead of the raw value, as a workaround
    /// for https://github.com/AaronO/serde_v8/issues/2.
    id_index: u32,
    property: LayoutBoxProperty,
  },
  Const {
    value: f64,
  },
  Add {
    left: Box<Real>,
    right: Box<Real>,
  },
  Sub {
    left: Box<Real>,
    right: Box<Real>,
  },
  Mul {
    left: Box<Real>,
    right: Box<Real>,
  },
  Div {
    left: Box<Real>,
    right: Box<Real>,
  },
  Select {
    cond: Box<Prop>,
    left: Box<Real>,
    right: Box<Real>,
  },
  MetricRef {
    index: usize,
  },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Prop {
  Eq { left: Box<Real>, right: Box<Real> },
  Lt { left: Box<Real>, right: Box<Real> },
  Le { left: Box<Real>, right: Box<Real> },
  Gt { left: Box<Real>, right: Box<Real> },
  Ge { left: Box<Real>, right: Box<Real> },
  Or { left: Box<Prop>, right: Box<Prop> },
  And { left: Box<Prop>, right: Box<Prop> },
  Not { value: Box<Prop> },
}

pub fn api_graphics_layout_solve(
  scope: &mut v8::HandleScope,
  args: v8::FunctionCallbackArguments,
  mut retval: v8::ReturnValue,
) -> Result<()> {
  let e = Executor::try_current_result()?.upgrade().unwrap();
  let request: LayoutRequest = v8_deserialize(scope, args.get(1))?;
  let z3_ctx = z3::Context::new(&z3::Config::new());
  let mut engine = LayoutEngine::new(scope, &z3_ctx);
  engine.build_idmap_idindex(&request.boxes);

  let mut metrics = Vec::with_capacity(request.metrics.len());
  for m in &request.metrics {
    metrics.push(engine.build_z3_real(scope, m)?);
  }
  engine.metrics = metrics;

  let mut constraints = Vec::with_capacity(request.constraints.len());
  for c in &request.constraints {
    constraints.push((engine.build_z3_prop(scope, &c.prop)?, c.weight));
  }

  let opt = z3::Optimize::new(&z3_ctx);
  for (c, w) in &constraints {
    if *w == 0 {
      opt.assert(c);
    } else {
      opt.assert_soft(c, *w, None);
    }
  }

  let check_res;

  // Cancellation.
  {
    struct Guard {
      handle: ManuallyDrop<z3::ContextHandle<'static>>,
      async_fence_tx: ManuallyDrop<mpsc::Sender<()>>,
    }
    impl Drop for Guard {
      fn drop(&mut self) {
        // Ensure that `handle` is dropped before `async_fence_tx`.
        unsafe {
          ManuallyDrop::drop(&mut self.handle);
          ManuallyDrop::drop(&mut self.async_fence_tx);
        }
      }
    }

    let handle = z3_ctx.handle();
    let mut cancel = e.get_cancel();
    let (async_fence_tx, async_fence_rx) = mpsc::channel::<()>();
    let guard = Guard {
      handle: ManuallyDrop::new(unsafe {
        std::mem::transmute::<z3::ContextHandle<'_>, z3::ContextHandle<'static>>(handle)
      }),
      async_fence_tx: ManuallyDrop::new(async_fence_tx),
    };

    // This future takes ownership of `guard` and will drop it.
    let handle = e.ctx.computation_watcher.spawn(async move {
      let _ = cancel.changed().await;
      guard.handle.interrupt();
    });

    check_res = opt.check(&[]);
    handle.abort();
    let _ = async_fence_rx.recv();
  }

  match check_res {
    z3::SatResult::Sat => {}
    z3::SatResult::Unsat => {
      retval.set(mk_v8_string(scope, "unsat")?.into());
      return Ok(());
    }
    z3::SatResult::Unknown => {
      retval.set(mk_v8_string(scope, "unknown")?.into());
      return Ok(());
    }
  }

  let model = opt
    .get_model()
    .expect("check returned sat but failed to get model");
  let mut sat = Vec::with_capacity(constraints.len());
  for (c, _) in &constraints {
    let x = model
      .eval(c, true)
      .expect("check returned sat but model does not provided value for a prop")
      .as_bool()
      .expect("failed to get value from a evaluated Bool");
    sat.push(x);
  }

  let mut boxes: Vec<LayoutBoxSolution> = Vec::with_capacity(request.boxes.len());
  for (id, bm) in &engine.idmap {
    boxes.push(LayoutBoxSolution {
      id: serde_v8::Value { v8_value: *id },
      left: must_eval_real(&model, &bm.left),
      right: must_eval_real(&model, &bm.right),
      top: must_eval_real(&model, &bm.top),
      bottom: must_eval_real(&model, &bm.bottom),
      z: must_eval_real(&model, &bm.z),
    });
  }
  let solution = LayoutSolution { boxes, sat };
  let solution = v8_serialize(scope, &solution)?;
  retval.set(solution);
  Ok(())
}

fn must_eval_real<'ctx>(model: &z3::Model<'ctx>, v: &z3::ast::Real<'ctx>) -> f64 {
  let (num, den) = model
    .eval(v, true)
    .expect("check returned sat but model does not provided value for a real")
    .as_real()
    .expect("failed to get value from a evaluated Real");
  num as f64 / den as f64
}

struct BoxMetrics<T> {
  left: T,
  right: T,
  top: T,
  bottom: T,
  z: T,
}

impl<T> BoxMetrics<T> {
  fn get(&self, prop: LayoutBoxProperty) -> &T {
    match prop {
      LayoutBoxProperty::Bottom => &self.bottom,
      LayoutBoxProperty::Left => &self.left,
      LayoutBoxProperty::Right => &self.right,
      LayoutBoxProperty::Top => &self.top,
      LayoutBoxProperty::Z => &self.z,
    }
  }
}

struct LayoutEngine<'s, 'ctx> {
  idmap: HashMap<v8::Local<'s, v8::Value>, BoxMetrics<z3::ast::Real<'ctx>>>,
  idindex: Vec<v8::Local<'s, v8::Value>>,
  metrics: Vec<z3::ast::Real<'ctx>>,
  z3_ctx: &'ctx z3::Context,
}

impl<'s, 'ctx> LayoutEngine<'s, 'ctx> {
  fn new(_scope: &mut v8::HandleScope<'s>, z3_ctx: &'ctx z3::Context) -> Self {
    Self {
      idmap: HashMap::new(),
      idindex: vec![],
      metrics: vec![],
      z3_ctx,
    }
  }

  fn build_idmap_idindex(&mut self, boxes: &[serde_v8::Value<'s>]) {
    for id in boxes {
      let (left, right, top, bottom, z) = (0..5)
        .map(|_| z3::ast::Real::fresh_const(&self.z3_ctx, "metric_"))
        .collect_tuple()
        .unwrap();
      self.idmap.insert(
        id.v8_value,
        BoxMetrics {
          left,
          right,
          top,
          bottom,
          z,
        },
      );
      self.idindex.push(id.v8_value);
    }
  }

  fn build_z3_prop(
    &self,
    scope: &mut v8::HandleScope<'s>,
    v: &Prop,
  ) -> Result<z3::ast::Bool<'ctx>> {
    Ok(match v {
      Prop::Eq { left, right } => self
        .build_z3_real(scope, left)?
        ._eq(&self.build_z3_real(scope, right)?),
      Prop::Lt { left, right } => self
        .build_z3_real(scope, left)?
        .lt(&self.build_z3_real(scope, right)?),
      Prop::Le { left, right } => self
        .build_z3_real(scope, left)?
        .le(&self.build_z3_real(scope, right)?),
      Prop::Gt { left, right } => self
        .build_z3_real(scope, left)?
        .gt(&self.build_z3_real(scope, right)?),
      Prop::Ge { left, right } => self
        .build_z3_real(scope, left)?
        .ge(&self.build_z3_real(scope, right)?),
      Prop::Or { left, right } => z3::ast::Bool::or(
        self.z3_ctx,
        &[
          &self.build_z3_prop(scope, left)?,
          &self.build_z3_prop(scope, right)?,
        ],
      ),
      Prop::And { left, right } => z3::ast::Bool::and(
        self.z3_ctx,
        &[
          &self.build_z3_prop(scope, left)?,
          &self.build_z3_prop(scope, right)?,
        ],
      ),
      Prop::Not { value } => self.build_z3_prop(scope, value)?.not(),
    })
  }

  fn build_z3_real(
    &self,
    scope: &mut v8::HandleScope<'s>,
    v: &Real,
  ) -> Result<z3::ast::Real<'ctx>> {
    #[derive(Error, Debug)]
    #[error("box index not found: {0}")]
    struct BoxIndexNotFound(u32);

    #[derive(Error, Debug)]
    #[error("box id not found: {0}")]
    struct BoxIdNotFound(String);

    #[derive(Error, Debug)]
    #[error("invalid real constant: {0}")]
    struct InvalidRealConst(f64);

    #[derive(Error, Debug)]
    #[error("invalid metric reference: {0}")]
    struct InvalidMetricRef(usize);

    Ok(match v {
      Real::BoxProperty { id_index, property } => {
        let id = *self
          .idindex
          .get(*id_index as usize)
          .ok_or(BoxIndexNotFound(*id_index))?;
        self
          .idmap
          .get(&id)
          .ok_or_else(|| BoxIdNotFound(id.to_rust_string_lossy(scope)))?
          .get(*property)
          .clone()
      }
      Real::Const { value } => {
        let frac = GenericFraction::<i32>::from(*value);
        if let (Some(num), Some(den)) = (frac.numer(), frac.denom()) {
          z3::ast::Real::from_real(self.z3_ctx, *num, *den)
        } else {
          return Err(InvalidRealConst(*value).into());
        }
      }
      Real::Add { left, right } => {
        self.build_z3_real(scope, left)? + self.build_z3_real(scope, right)?
      }
      Real::Sub { left, right } => {
        self.build_z3_real(scope, left)? - self.build_z3_real(scope, right)?
      }
      Real::Mul { left, right } => {
        self.build_z3_real(scope, left)? * self.build_z3_real(scope, right)?
      }
      Real::Div { left, right } => {
        self.build_z3_real(scope, left)? / self.build_z3_real(scope, right)?
      }
      Real::Select { cond, left, right } => self.build_z3_prop(scope, cond)?.ite(
        &self.build_z3_real(scope, &left)?,
        &self.build_z3_real(scope, right)?,
      ),
      Real::MetricRef { index } => self
        .metrics
        .get(*index)
        .ok_or(InvalidMetricRef(*index))?
        .clone(),
    })
  }
}
