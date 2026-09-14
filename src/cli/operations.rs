//! CLI adapter: render backend results; domain validation lives in the library.
use anyhow::Result;
use std::path::PathBuf;
use loadbot::{operations as backend, paths::Paths, catalog::ResolvedTool};
use super::output;

pub fn catalog_add(paths: &Paths, name: &str, url: String, writable: bool) -> Result<()> {
    output::with_context(|context| backend::catalog_add(paths, name, url, writable, context)).map(|_| ())
}

pub fn catalog_initialize(paths: &Paths,
    name: &str,
    url: String,
    writable: bool,
    commit: bool,
    push: bool,
) -> Result<()> {
    output::with_context(|context| backend::catalog_initialize(paths, name, url, writable, commit, push, context)).map(|_| ())
}

pub fn catalog_list(paths: &Paths) -> Result<()> {
    output::with_context(|context| backend::catalog_list(paths, context)).map(|_| ())
}

pub fn catalog_sync(paths: &Paths, name: &str) -> Result<()> {
    output::with_context(|context| backend::catalog_sync(paths, name, context)).map(|_| ())
}

pub fn catalog_status(paths: &Paths, name: &str) -> Result<()> {
    output::with_context(|context| backend::catalog_status(paths, name, context)).map(|_| ())
}

pub fn catalog_path(paths: &Paths, name: &str) -> Result<()> {
    println!("{}", backend::catalog_path(paths, name)?.display());
    Ok(())
}

pub fn catalog_migrate(paths: &Paths, name: &str, url: String) -> Result<()> {
    output::with_context(|context| backend::catalog_migrate(paths, name, url, context)).map(|_| ())
}

pub fn tool_add(paths: &Paths,
    catalog_name: &str,
    name: &str,
    url: String,
    revision: Option<String>,
    commit: bool,
    push: bool,
) -> Result<()> {
    output::with_context(|context| backend::tool_add(paths, catalog_name, name, url, revision, commit, push, context)).map(|_| ())
}

pub fn tool_list(paths: &Paths) -> Result<()> {
    output::with_context(|context| backend::tool_list(paths, context)).map(|_| ())
}

pub fn tool_pull(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    output::with_context(|context| backend::tool_pull(paths, name, catalog_name, context)).map(|_| ())
}

pub fn tool_update(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    output::with_context(|context| backend::tool_update(paths, name, catalog_name, context)).map(|_| ())
}

pub fn tool_status(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    output::with_context(|context| backend::tool_status(paths, name, catalog_name, context)).map(|_| ())
}

pub fn tool_path(paths: &Paths, name: &str, catalog_name: Option<&str>) -> Result<()> {
    println!("{}", output::with_context(|context| backend::tool_path(paths, name, catalog_name, context))?.display());
    Ok(())
}

pub fn installed_tool_path(paths: &Paths, name: &str, catalog_name: &str) -> Result<PathBuf> {
    output::with_context(|context| backend::installed_tool_path(paths, name, catalog_name, context))
}

pub fn installed_tools(paths: &Paths) -> Result<Vec<ResolvedTool>> {
    output::with_context(|context| backend::installed_tools(paths, context))
}

pub fn all_tools(paths: &Paths) -> Result<Vec<ResolvedTool>> {
    output::with_context(|context| backend::all_tools(paths, context))
}

pub fn writable_catalogs(paths: &Paths) -> Result<Vec<String>> {
    output::with_context(|context| backend::writable_catalogs(paths, context))
}

pub fn default_writable_catalog(paths: &Paths) -> Result<Option<String>> {
    output::with_context(|context| backend::default_writable_catalog(paths, context))
}

pub fn catalog_names(paths: &Paths) -> Result<Vec<String>> {
    backend::catalog_names(paths)
}

pub fn available_catalog_names(paths: &Paths) -> Result<Vec<String>> {
    output::with_context(|context| backend::available_catalog_names(paths, context))
}
