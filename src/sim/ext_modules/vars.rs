//! Declared variables (RFC 0001 section 6e; appendix "IDs", "Custom
//! sections" and 8.3): the `vera.vars` declarations of the loaded modules and
//! their values.
//!
//! [`VarStore`] is the one owner. IDs start at 1000 in load order: the mod
//! module's declarations, then the map module's, each in declaration order.
//! The declarations come from the module bytes, so a save carries only the
//! values; a value equal to its default is not stored. Object-scope values
//! go with their object ([`VarStore::forget_object`]). The Red Alert 2
//! variables (IDs 0..149) stay with the trigger runtime.
//!
//! Module triggers' `Where` variable clauses read these values and their
//! `SetVar` action writes them through the host's `var_set`.
//!
//! Residual: a write does not yet reset the timers of triggers waiting on the
//! variable (`VariablePoll`, not raised yet).

use std::collections::BTreeMap;

use vera20k_wasm_ext::{Invalid, Status, VarDecl, VarScope, Visibility};

use super::{ModuleSource, Ref, decode_ref};

/// The first declared variable's ID.
pub const FIRST_DECLARED_VAR: i32 = 1000;

/// What one variable value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Holder {
    Global,
    House(u32),
    Object(u64),
}

/// The declared variables' values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct VarValues {
    pub(super) global: BTreeMap<i32, i64>,
    pub(super) house: BTreeMap<(i32, u32), i64>,
    pub(super) object: BTreeMap<(u64, i32), i64>,
}

#[derive(Debug, Clone, Default)]
pub struct VarStore {
    /// By ID from [`FIRST_DECLARED_VAR`]; rebuilt from the modules.
    decls: Vec<(ModuleSource, VarDecl)>,
    pub(super) values: VarValues,
}

impl VarStore {
    /// Sets the declarations from the loaded modules, in load order.
    pub(super) fn declare<'a>(
        &mut self,
        modules: impl Iterator<Item = (ModuleSource, &'a [VarDecl])>,
    ) {
        self.decls = modules
            .flat_map(|(source, decls)| decls.iter().map(move |d| (source, d.clone())))
            .collect();
    }

    fn decl(&self, var: i32) -> Option<&VarDecl> {
        let index = usize::try_from(var.checked_sub(FIRST_DECLARED_VAR)?).ok()?;
        self.decls.get(index).map(|(_, decl)| decl)
    }

    /// `lookup(Variable, name)` for a module of origin `own`: the `mod.` and
    /// `map.` prefixes address that origin's variables.
    pub(super) fn lookup(&self, own: ModuleSource, name: &str) -> Option<i32> {
        let prefixed = |prefix: &str| {
            name.get(..prefix.len())
                .filter(|p| p.eq_ignore_ascii_case(prefix))
                .map(|_| &name[prefix.len()..])
        };
        let (origin, name) = match (prefixed("mod."), prefixed("map.")) {
            (Some(rest), _) => (ModuleSource::Mod, rest),
            (_, Some(rest)) => (ModuleSource::Map, rest),
            _ => (own, name),
        };
        let index = self
            .decls
            .iter()
            .position(|(source, decl)| *source == origin && decl.name.eq_ignore_ascii_case(name))?;
        i32::try_from(index).ok()?.checked_add(FIRST_DECLARED_VAR)
    }

    /// A declared variable's scope.
    pub(super) fn scope(&self, var: i32) -> Option<VarScope> {
        self.decl(var).map(|decl| decl.scope)
    }

    /// A declared variable's client visibility (RFC 6e), which client
    /// modules read it under; the simulation reads every variable.
    pub(crate) fn visibility(&self, var: i32) -> Option<Visibility> {
        self.decl(var).map(|decl| decl.visibility)
    }

    /// A declared variable's name.
    pub(super) fn name(&self, var: i32) -> Option<&str> {
        self.decl(var).map(|decl| decl.name.as_str())
    }

    /// The holder `scope` names for `var`, checked against its declaration;
    /// `None` when that house or object is gone. A wrong scope or an unknown
    /// variable is invalid.
    pub(super) fn holder(
        &self,
        scope: i64,
        var: i32,
        house_exists: impl Fn(usize) -> bool,
        object_exists: impl Fn(u64) -> bool,
    ) -> Result<Option<Holder>, Invalid> {
        let decl = self
            .decl(var)
            .ok_or_else(|| Invalid(format!("unknown variable {var}")))?;
        let wrong = || {
            Invalid(format!(
                "variable {} has {:?} scope, not {scope:#x}",
                decl.name, decl.scope
            ))
        };
        Ok(match (decl.scope, decode_ref(scope)?) {
            (VarScope::Global, None) => Some(Holder::Global),
            (VarScope::House, Some(Ref::House(index))) => house_exists(index)
                .then(|| u32::try_from(index).ok().map(Holder::House))
                .flatten(),
            (VarScope::Object, Some(Ref::Object(id))) => {
                object_exists(id).then_some(Holder::Object(id))
            }
            _ => return Err(wrong()),
        })
    }

    pub(super) fn get(&self, var: i32, holder: Holder) -> i64 {
        let stored = match holder {
            Holder::Global => self.values.global.get(&var),
            Holder::House(house) => self.values.house.get(&(var, house)),
            Holder::Object(id) => self.values.object.get(&(id, var)),
        };
        stored
            .copied()
            .unwrap_or_else(|| self.decl(var).map_or(0, |decl| decl.default))
    }

    pub(super) fn set(&mut self, var: i32, holder: Holder, value: i64) {
        let default = self.decl(var).map_or(0, |decl| decl.default);
        let keep = (value != default).then_some(value);
        let values = &mut self.values;
        match holder {
            Holder::Global => put(&mut values.global, var, keep),
            Holder::House(house) => put(&mut values.house, (var, house), keep),
            Holder::Object(id) => put(&mut values.object, (id, var), keep),
        }
    }

    /// The declarations without a value.
    pub(super) fn declarations_only(&self) -> Self {
        Self {
            decls: self.decls.clone(),
            values: VarValues::default(),
        }
    }

    /// `lookup` for the client side: a declared variable's ID by name for a
    /// reader of origin `own` (the `mod.` and `map.` prefixes address that
    /// origin's).
    pub fn find(&self, own: ModuleSource, name: &str) -> Option<i32> {
        self.lookup(own, name)
    }

    /// A declared variable's scope, visibility and default.
    pub fn declared(&self, var: i32) -> Option<(VarScope, Visibility, i64)> {
        self.decl(var)
            .map(|decl| (decl.scope, decl.visibility, decl.default))
    }

    /// The object-scope values `id` holds apart from their defaults, by
    /// variable ID.
    pub fn object_values(&self, id: u64) -> impl Iterator<Item = (i32, i64)> + '_ {
        self.values
            .object
            .range((id, i32::MIN)..=(id, i32::MAX))
            .map(|((_, var), value)| (*var, *value))
    }

    /// Drops a removed object's values.
    pub(super) fn forget_object(&mut self, id: u64) {
        let keys: Vec<_> = self
            .values
            .object
            .range((id, i32::MIN)..=(id, i32::MAX))
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            self.values.object.remove(&key);
        }
    }

    /// The `var_get` answer: the value, or 0 and `Gone`.
    pub(super) fn read(&self, var: i32, holder: Option<Holder>) -> (i64, Status) {
        holder.map_or((0, Status::Gone), |holder| {
            (self.get(var, holder), Status::Ok)
        })
    }
}

fn put<K: Ord>(map: &mut BTreeMap<K, i64>, key: K, value: Option<i64>) {
    match value {
        Some(value) => {
            map.insert(key, value);
        }
        None => {
            map.remove(&key);
        }
    }
}
