; Java syntax evidence. Method names are not resolved call targets.
(class_declaration name: (_) @name) @declares.type
(interface_declaration name: (_) @name) @declares.type
(enum_declaration name: (_) @name) @declares.type
(record_declaration name: (_) @name) @declares.type
(annotation_type_declaration name: (_) @name) @declares.type
(method_declaration name: (_) @name) @declares.method @container
(constructor_declaration name: (_) @name) @declares.constructor @container
(compact_constructor_declaration name: (_) @name) @declares.constructor @container

; A local or anonymous type's initializer is not an invocation by the outer method.
(class_body) @boundary
(enum_body) @boundary
(interface_body) @boundary
(annotation_type_body) @boundary

(method_invocation name: (_) @name) @invokes
; `Type::method`, `obj::method`, `this::method` pass the method; `Type::new` names none.
(method_reference "::" (identifier) @name) @references

((import_declaration) @imports.static (#has? @imports.static "static"))
((import_declaration) @imports (#not-has? @imports "static"))
