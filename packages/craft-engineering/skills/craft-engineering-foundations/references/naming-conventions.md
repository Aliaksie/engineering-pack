# Name the domain and contract

Use the consumer's vocabulary and language conventions. A reader should be able to infer
what a value represents, its meaningful units and whether an operation can produce an effect.
Prefer a concrete role over a vague container name when the role is known.

Distinguish values that share a representation but differ in meaning: an elapsed duration is
not a wall-clock timestamp, and an external account identifier is not an authenticated actor.
Represent units or domains in types when useful, otherwise make them visible in names or
boundary validation. Avoid inventing a naming scheme that conflicts with an established API.

Renaming a public identifier can affect serialization, URLs, configuration, persisted state,
reflection and consumers outside the checkout. Identify those surfaces before treating a
rename as mechanical. Preserve a required external spelling at its boundary and translate
locally when that improves clarity.

Check names in error messages, tests and documentation with the code. Prefer a short name
in a narrow obvious scope; add specificity where readers otherwise need distant context.
