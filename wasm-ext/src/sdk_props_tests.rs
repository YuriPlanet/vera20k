//! The Rust SDK's property constants match [`crate::PROPERTIES`].

/// `sdk/rust/vera20k-sdk/src/props.rs` holds one `pub const Name: Prop =
/// Prop(id);` per property, a `.` in the name written `_`
/// (`Passable.Foot` is `Passable_Foot`); any other drift from the engine's
/// table fails.
#[test]
fn the_sdk_numbers_every_property_as_the_engine_does() {
    let source = include_str!("../../sdk/rust/vera20k-sdk/src/props.rs");
    let constants: Vec<(String, usize)> = source
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("pub const ")?;
            let (name, rest) = rest.split_once(": Prop = Prop(")?;
            let id = rest.strip_suffix(");")?.parse().expect("a numeric ID");
            Some((name.to_owned(), id))
        })
        .collect();
    let expected: Vec<(String, usize)> = crate::PROPERTIES
        .iter()
        .enumerate()
        .map(|(id, name)| (name.replace('.', "_"), id))
        .collect();
    assert_eq!(constants, expected);
}

/// The TypeScript SDK's generated `sdk/typescript/assembly/generated/props.ts`
/// holds one `export const Name: Prop = id;` per property, named as the Rust
/// SDK names them (`npm run tables` regenerates it); any drift from the
/// engine's table fails.
#[test]
fn the_typescript_sdk_numbers_every_property_as_the_engine_does() {
    let source = include_str!("../../sdk/typescript/assembly/generated/props.ts");
    let constants: Vec<(String, usize)> = source
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("export const ")?;
            let (name, rest) = rest.split_once(": Prop = ")?;
            let id = rest.strip_suffix(';')?.parse().expect("a numeric ID");
            Some((name.to_owned(), id))
        })
        .collect();
    let expected: Vec<(String, usize)> = crate::PROPERTIES
        .iter()
        .enumerate()
        .map(|(id, name)| (name.replace('.', "_"), id))
        .collect();
    assert_eq!(constants, expected);
}
