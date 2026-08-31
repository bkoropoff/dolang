use crate::global::Global;
use dolang::{
    compile::Compiler,
    extension,
    extension::{Extension, Version},
    runtime::vm::Builder,
};
use std::convert::Infallible;

pub struct WinnetExt;
impl Extension for WinnetExt {
    type Error = Infallible;
    const NAME: &str = "dolang-winnet";
    const VERSION: Version = dolang::package_version!();
    const DESCRIPTION: &str = "Do Windows NetAPI Extension";
    const DEPENDS: &'static [&'static str] = &[<dolang_ext_shell::Shell as Extension>::NAME];
    fn apply_compiler(&self, _compiler: &mut Compiler) -> Result<(), Self::Error> {
        Ok(())
    }
    fn apply_vm<'v>(&self, builder: &mut Builder<'v>) -> Result<(), Self::Error> {
        let global = Global::new(builder);
        let global = builder.register_state(global);
        crate::user::configure_vm(builder, global);
        Ok(())
    }
}
extension!(WinnetExt);
