# Conventions

This specification captures implementation practices that deliberately push correctness into the compiler and type system instead of depending on comments, conventions, or ad hoc runtime checks.

## Type-Driven Design

convention[convention.invariants.encode-in-types]
Implementation invariants that can be represented in Rust types should be encoded in those types so incorrect states are rejected at compile time when practical.

convention[convention.types.newtypes-for-domain-boundaries]
Domain boundaries such as window ownership, thread affinity, coordinate spaces, and terminal cell positions should prefer dedicated newtypes over raw primitives in internal APIs when practical.

## Units And Spatial Math

convention[convention.measurements.use-uom]
Dimensioned measurements should prefer `uom` quantities over bare numeric values when physical units or pixel-space quantities would otherwise be implicit.

convention[convention.spatial.transforms.use-sguaba]
Coordinate-space conversions should prefer `sguaba` typed spaces and transforms when that lets the compiler reject mixing incompatible coordinate systems.

## Intent

These conventions describe the direction of the codebase rather than demanding an all-at-once rewrite. During refactors, new code and touched code should move toward these practices when the resulting types remain understandable and proportionate.