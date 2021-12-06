extern crate prost_build;

fn main() {
  prost_build::compile_protos(&["mdsproto/mds.proto"], &["mdsproto/"]).unwrap();
}
