import {
  CanvasConfig,
  CanvasDrawConfig,
  CanvasEncodeConfig,
  CanvasOp,
  CanvasPathOp,
  CanvasRenderSvgConfig,
  CanvasRenderSvgFitTo,
} from "../native_schema";

function createRasterFromConfig(config: CanvasConfig): Uint8Array {
  let pixelSize: number;
  switch (config.color_type) {
    case "Alpha8":
    case "Gray8":
      pixelSize = 1;
      break;
    case "RGB565":
    case "ARGB4444":
      pixelSize = 2;
      break;
    case "RGBA8888":
    case "RGB888x":
    case "BGRA8888":
    case "RGBA1010102":
    case "BGRA1010102":
    case "RGB101010x":
    case "BGR101010x":
      pixelSize = 4;
      break;
    case "RGBAF16Norm":
    case "RGBAF16":
      pixelSize = 8;
      break;
    default:
      throw new Error("unsupported color type: " + config.color_type);
  }

  return new Uint8Array(
    pixelSize * config.dimensions.width * config.dimensions.height
  );
}

class SkiaCanvas {
  config: CanvasConfig;
  log: CanvasOp[] = [];
  raster: Uint8Array;
  strokeStyle: string | undefined = undefined;
  fillStyle: string | undefined = undefined;
  lineWidth: number | undefined = undefined;
  font: string = "10px sans-serif";

  constructor(config: CanvasConfig) {
    this.config = config;
    this.raster = createRasterFromConfig(config);
  }

  commit(): void {
    const log = this.log;
    this.log = [];

    // Start point for the next `commit()`
    if (this.strokeStyle !== undefined) {
      this.log.push({
        op: "SetStrokeStyleColor",
        color: this.strokeStyle,
      });
    }
    if (this.fillStyle !== undefined) {
      this.log.push({
        op: "SetFillStyleColor",
        color: this.fillStyle,
      });
    }
    if (this.lineWidth !== undefined) {
      this.log.push({
        op: "SetStrokeLineWidth",
        width: this.lineWidth,
      });
    }
    if (this.font !== undefined) {
      this.log.push({
        op: "SetFont",
        font: this.font,
      });
    }

    __blueboat_host_invoke(
      "graphics_canvas_commit",
      this.config,
      log,
      this.raster
    );
  }

  encode(config: CanvasEncodeConfig): Uint8Array {
    this.commit();
    return <Uint8Array>(
      __blueboat_host_invoke(
        "graphics_canvas_encode",
        this.config,
        config,
        this.raster
      )
    );
  }

  drawFrom(that: SkiaCanvas, config: CanvasDrawConfig) {
    that.commit();
    this.commit();
    __blueboat_host_invoke(
      "graphics_canvas_draw",
      that.config,
      that.raster,
      this.config,
      this.raster,
      config
    );
  }

  renderSvg(svg: string, fit: CanvasRenderSvgFitTo) {
    this.commit();
    let config: CanvasRenderSvgConfig = {
      dimensions: this.config.dimensions,
      fit_to: fit,
    };
    __blueboat_host_invoke(
      "graphics_canvas_render_svg",
      svg,
      config,
      this.raster
    );
  }
}

export class Path2DImpl implements Path2D {
  _pathBuffer: CanvasPathOp[] = [];

  addPath(path: Path2DImpl, transform?: DOMMatrix2DInit): void {
    this._pathBuffer.push({
      op: "AddPath",
      subpath: path._pathBuffer.slice(),
      matrix:
        transform &&
        transform.a !== undefined &&
        transform.b !== undefined &&
        transform.c !== undefined &&
        transform.d !== undefined &&
        transform.e !== undefined &&
        transform.f !== undefined
          ? {
              a: transform.a,
              b: transform.b,
              c: transform.c,
              d: transform.d,
              e: transform.e,
              f: transform.f,
            }
          : null,
    });
  }
  arc(
    x: number,
    y: number,
    radius: number,
    startAngle: number,
    endAngle: number,
    anticlockwise?: boolean
  ): void {
    this._pathBuffer.push({
      op: "Arc",
      x,
      y,
      radius,
      start_angle: startAngle,
      end_angle: endAngle,
      counterclockwise: !!anticlockwise,
    });
  }
  arcTo(x1: number, y1: number, x2: number, y2: number, radius: number): void {
    this._pathBuffer.push({
      op: "ArcTo",
      x1,
      y1,
      x2,
      y2,
      radius,
    });
  }
  bezierCurveTo(
    cp1x: number,
    cp1y: number,
    cp2x: number,
    cp2y: number,
    x: number,
    y: number
  ): void {
    this._pathBuffer.push({
      op: "BezierCurveTo",
      cp1x,
      cp1y,
      cp2x,
      cp2y,
      x,
      y,
    });
  }
  closePath(): void {
    this._pathBuffer.push({
      op: "Close",
    });
  }
  ellipse(
    x: number,
    y: number,
    radiusX: number,
    radiusY: number,
    rotation: number,
    startAngle: number,
    endAngle: number,
    anticlockwise?: boolean
  ): void {
    this._pathBuffer.push({
      op: "Ellipse",
      x,
      y,
      radius_x: radiusX,
      radius_y: radiusY,
      rotation,
      start_angle: startAngle,
      end_angle: endAngle,
      counterclockwise: !!anticlockwise,
    });
  }
  lineTo(x: number, y: number): void {
    this._pathBuffer.push({
      op: "LineTo",
      x,
      y,
    });
  }
  moveTo(x: number, y: number): void {
    this._pathBuffer.push({
      op: "MoveTo",
      x,
      y,
    });
  }
  quadraticCurveTo(cpx: number, cpy: number, x: number, y: number): void {
    this._pathBuffer.push({
      op: "QuadraticCurveTo",
      cpx,
      cpy,
      x,
      y,
    });
  }
  rect(x: number, y: number, width: number, height: number): void {
    this._pathBuffer.push({
      op: "Rect",
      x,
      y,
      width,
      height,
    });
  }
}

export class CanvasImpl {
  impl: SkiaCanvas;

  constructor(width: number, height: number) {
    this.impl = new SkiaCanvas({
      alpha_type: "Opaque",
      color_type: "RGBA8888",
      dimensions: {
        width,
        height,
      },
      pixel_geometry: "RGBH",
    });
  }

  get width() {
    return this.impl.config.dimensions.width;
  }

  get height() {
    return this.impl.config.dimensions.height;
  }

  peekPixels(): Uint8Array {
    return this.impl.raster;
  }

  flush() {
    this.impl.commit();
  }

  encode(options?: { type?: string; quality?: number }): Uint8Array {
    return this.impl.encode({
      format: options?.type == "jpeg" ? "JPEG" : "PNG",
      quality: options?.quality || 90,
    });
  }

  getContext(contextId: string, options?: any): CanvasRenderingContext2DImpl {
    if (contextId === "2d") {
      return new CanvasRenderingContext2DImpl(this.impl);
    } else {
      throw new Error("context type not supported");
    }
  }
}

export class CanvasRenderingContext2DImpl {
  private impl: SkiaCanvas;
  private path: Path2DImpl = new Path2DImpl();

  constructor(impl: SkiaCanvas) {
    this.impl = impl;
  }
  beginPath(): void {
    this.path._pathBuffer = [];
  }
  fill(fillRule?: CanvasFillRule): void;
  fill(path: Path2D, fillRule?: CanvasFillRule): void;
  fill(path?: any, fillRule?: any): void {
    if (path instanceof Path2DImpl) {
      this.impl.log.push({
        op: "Fill",
        path: path._pathBuffer.slice(),
        fill_rule: fillRule === "evenodd" ? "evenodd" : "nonzero",
      });
    } else {
      if (typeof path !== "string") {
        throw new TypeError("bad args");
      }
      this.impl.log.push({
        op: "Fill",
        path: this.path._pathBuffer.slice(),
        fill_rule: path == "evenodd" ? "evenodd" : "nonzero",
      });
    }
  }
  stroke(): void;
  stroke(path: Path2D): void;
  stroke(path?: any): void {
    if (path instanceof Path2DImpl) {
      this.impl.log.push({
        op: "Stroke",
        path: path._pathBuffer.slice(),
      });
    } else {
      this.impl.log.push({
        op: "Stroke",
        path: this.path._pathBuffer.slice(),
      });
    }
  }

  get fillStyle(): string | CanvasGradient | CanvasPattern {
    return this.impl.fillStyle || "";
  }

  set fillStyle(value: string | CanvasGradient | CanvasPattern) {
    if (typeof value !== "string") {
      throw new TypeError("fillStyle: unsupported type");
    }
    this.impl.fillStyle = value;
    this.impl.log.push({
      op: "SetFillStyleColor",
      color: value,
    });
  }

  get strokeStyle(): string | CanvasGradient | CanvasPattern {
    return this.impl.strokeStyle || "";
  }

  set strokeStyle(value: string | CanvasGradient | CanvasPattern) {
    if (typeof value !== "string") {
      throw new TypeError("strokeStyle: unsupported type");
    }
    this.impl.strokeStyle = value;
    this.impl.log.push({
      op: "SetStrokeStyleColor",
      color: value,
    });
  }

  get lineWidth(): number {
    return this.impl.lineWidth || 0;
  }

  set lineWidth(x: number) {
    this.impl.lineWidth = x;
    this.impl.log.push({
      op: "SetStrokeLineWidth",
      width: x,
    });
  }

  get font(): string {
    return this.impl.font;
  }

  set font(x: string) {
    this.impl.font = x;
    this.impl.log.push({
      op: "SetFont",
      font: x,
    });
  }

  arc(
    x: number,
    y: number,
    radius: number,
    startAngle: number,
    endAngle: number,
    anticlockwise?: boolean
  ): void {
    this.path.arc(x, y, radius, startAngle, endAngle, anticlockwise);
  }
  arcTo(x1: number, y1: number, x2: number, y2: number, radius: number): void {
    this.path.arcTo(x1, y1, x2, y2, radius);
  }
  bezierCurveTo(
    cp1x: number,
    cp1y: number,
    cp2x: number,
    cp2y: number,
    x: number,
    y: number
  ): void {
    this.path.bezierCurveTo(cp1x, cp1y, cp2x, cp2y, x, y);
  }
  closePath(): void {
    this.path.closePath();
  }
  ellipse(
    x: number,
    y: number,
    radiusX: number,
    radiusY: number,
    rotation: number,
    startAngle: number,
    endAngle: number,
    anticlockwise?: boolean
  ): void {
    this.path.ellipse(
      x,
      y,
      radiusX,
      radiusY,
      rotation,
      startAngle,
      endAngle,
      anticlockwise
    );
  }
  lineTo(x: number, y: number): void {
    this.path.lineTo(x, y);
  }
  moveTo(x: number, y: number): void {
    this.path.moveTo(x, y);
  }
  quadraticCurveTo(cpx: number, cpy: number, x: number, y: number): void {
    this.path.quadraticCurveTo(cpx, cpy, x, y);
  }
  rect(x: number, y: number, w: number, h: number): void {
    this.path.rect(x, y, w, h);
  }
  clearRect(x: number, y: number, w: number, h: number): void {
    this.impl.log.push({
      op: "ClearRect",
      rect: {
        left: x,
        right: x + w,
        top: y,
        bottom: y + h,
      },
    });
  }
  fillRect(x: number, y: number, w: number, h: number): void {
    let p = new Path2DImpl();
    p.rect(x, y, w, h);
    this.fill(p);
  }
  strokeRect(x: number, y: number, w: number, h: number): void {
    let p = new Path2DImpl();
    p.rect(x, y, w, h);
    this.stroke(p);
  }
  fillText(text: string, x: number, y: number) {
    this.impl.log.push({
      op: "FillText",
      x,
      y,
      text,
    });
  }

  renderSvg(svg: string, fit: CanvasRenderSvgFitTo) {
    this.impl.renderSvg(svg, fit);
  }

  private drawImage_simple(
    that: CanvasImpl,
    dx: number,
    dy: number,
    dWidth?: number,
    dHeight?: number
  ) {
    this.impl.drawFrom(that.impl, {
      sx: 0,
      sy: 0,
      dx,
      dy,
      dw: dWidth,
      dh: dHeight,
    });
  }

  private drawImage_complex(
    that: CanvasImpl,
    sx: number,
    sy: number,
    sWidth: number,
    sHeight: number,
    dx: number,
    dy: number,
    dWidth?: number,
    dHeight?: number
  ) {
    this.impl.drawFrom(that.impl, {
      sx,
      sy,
      sw: sWidth,
      sh: sHeight,
      dx,
      dy,
      dw: dWidth,
      dh: dHeight,
    });
  }

  drawImage(that: CanvasImpl, dx: number, dy: number): void;
  drawImage(
    that: CanvasImpl,
    dx: number,
    dy: number,
    dWidth: number,
    dHeight: number
  ): void;
  drawImage(
    that: CanvasImpl,
    sx: number,
    sy: number,
    sWidth: number,
    sHeight: number,
    dx: number,
    dy: number,
    dWidth: number,
    dHeight: number
  ): void;
  drawImage(that: CanvasImpl, ...args: any[]): void {
    if (args.length == 2) {
      this.drawImage_simple(that, args[0], args[1]);
    } else if (args.length == 4) {
      this.drawImage_simple(that, args[0], args[1], args[2], args[3]);
    } else if (args.length == 8) {
      this.drawImage_complex(
        that,
        args[0],
        args[1],
        args[2],
        args[3],
        args[4],
        args[5],
        args[6],
        args[7]
      );
    } else {
      throw new Error("invalid args");
    }
  }
}
