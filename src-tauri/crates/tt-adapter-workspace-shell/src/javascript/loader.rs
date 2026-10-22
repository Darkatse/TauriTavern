use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{Ctx, Error, Module, Result};

use super::files::{Files, resolve_path};
use super::runtime::RUNTIME_MODULE;

pub(super) struct ModuleResolver;

impl Resolver for ModuleResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        if attributes.is_some() {
            return Err(Error::new_resolving_message(
                base,
                name,
                "Import attributes are not supported; import a .js/.mjs module.",
            ));
        }
        if name == RUNTIME_MODULE
            || crate::kit::MODULES
                .iter()
                .any(|(module, _)| *module == name)
        {
            return Ok(name.to_owned());
        }
        if !name.starts_with(['/', '.']) {
            return Err(Error::new_resolving_message(
                base,
                name,
                "This module is not available. Import a workspace .js/.mjs file or a built-in module listed in js --help.",
            ));
        }
        let directory = base
            .rsplit_once('/')
            .map_or("/", |(directory, _)| directory);
        let path = resolve_path(directory, name)
            .map_err(|message| Error::new_resolving_message(base, name, message))?;
        ensure_module_path(&path)
            .map_err(|message| Error::new_resolving_message(base, name, message))?;
        Ok(path)
    }
}

pub(super) struct ModuleLoader(pub Files);

impl Loader for ModuleLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js>> {
        if let Some((_, source)) = crate::kit::MODULES
            .iter()
            .find(|(module, _)| *module == name)
        {
            self.0
                .charge_input(source.len())
                .map_err(|message| Error::new_loading_message(name, message))?;
            return Module::declare(ctx.clone(), name, *source);
        }
        let source = self
            .0
            .read(name)
            .map_err(|message| Error::new_loading_message(name, message))?;
        Module::declare(ctx.clone(), name, source)
    }
}

pub(super) fn ensure_module_path(path: &str) -> std::result::Result<(), String> {
    if path.ends_with(".js") || path.ends_with(".mjs") {
        Ok(())
    } else {
        Err(format!(
            "`{path}` must be an explicit .js or .mjs module path."
        ))
    }
}
