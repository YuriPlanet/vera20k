// Variables declared in `vera.json` (RFC 0001 section 6e). The build writes
// the declarations to the module's `vera.vars` section and generates
// `src/generated/vars.ts`, which makes one variable per declaration:
//
//   export const Wave = new GlobalVar("Wave");
//   export const Panicked = new ObjectVar("Panicked");
//
// Each access is one `var_get` or `var_set`; the ID is looked up once.

export { Var, GlobalVar, HouseVar, ObjectVar } from "./core";

/** Where a declared variable lives (`Scope`, appendix 8.3). */
export enum Scope {
  Global = 0,
  House = 1,
  Object = 2,
}

/** Who may read a declared variable (`Visibility`, appendix 8.3). */
export enum Visibility {
  Private = 0,
  Allies = 1,
  InSight = 2,
  Public = 3,
}
