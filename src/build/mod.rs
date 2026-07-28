mod builder;
mod plan;
mod reporter;
mod rollout;
mod run;
mod vars;

#[cfg(test)]
mod tests;

pub use builder::ensure_builder;
pub use rollout::project_rollout;
pub use run::{project_build, project_push};
pub use vars::{interpolate, vars_for, BuildOverrides, Vars};

pub(crate) use vars::resolve_root;
