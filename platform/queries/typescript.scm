; TypeScript only.
(public_field_definition
  name: [(property_identifier) (private_property_identifier)] @name
  value: [(arrow_function) (function_expression)]) @declares.method @container
(abstract_class_declaration name: (_) @name) @declares.class
(interface_declaration name: (_) @name) @declares.interface
(type_alias_declaration name: (_) @name) @declares.type
(enum_declaration name: (_) @name) @declares.enum
(internal_module name: (_) @name) @declares.module
(function_signature name: (_) @name) @declares.function
(method_signature name: (_) @name) @declares.method
(abstract_method_signature name: (_) @name) @declares.method
