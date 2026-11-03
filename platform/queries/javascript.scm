; JavaScript syntax evidence: no claims about runtime calls or package resolution.
(function_declaration name: (identifier) @name) @declares
(generator_function_declaration name: (identifier) @name) @declares
(class_declaration name: (identifier) @name) @declares
(variable_declarator name: (identifier) @name) @declares

(import_statement source: (string) @target) @imports
(export_statement source: (string) @target) @imports
(call_expression function: (import)) @dynamic-import
