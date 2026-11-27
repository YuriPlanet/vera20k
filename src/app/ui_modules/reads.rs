//! Client reads (`client_get`, `client_get_many`, `client_find`,
//! `client_var_get`, `client_lookup`, `client_name_of`,
//! `client_has_kind`): only what this player's own interface can show
//! (RFC 10, "Client reads"). They are the simulation's own reads behind a
//! visibility gate, so they live beside them in
//! [`crate::sim::ext_modules::client_reads`].

pub(crate) use crate::sim::ext_modules::client_reads::*;
