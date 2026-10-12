use std::collections::HashSet;

use super::{Checker, typed};
use crate::ast::{BinaryOp, ParamMode};
use crate::hir::{self, Const, Expr, ExprKind, Stmt, StmtKind};
use crate::resolve::{FieldId, FunctionId, LocalId, LocalKind};
use crate::source::Span;
use crate::types::{TypeId, TypeKind, TypeStore};

impl Checker<'_> {
    pub(super) fn json_type_argument(&self, function: FunctionId) -> bool {
        let index = function.0 as usize;
        self.res.packages[self.res.function_package[index]].path == "std/encoding/json"
            && matches!(
                self.res.functions[index].name.text.as_str(),
                "Marshal"
                    | "MarshalWith"
                    | "MarshalIndent"
                    | "Unmarshal"
                    | "UnmarshalWith"
                    | "UnmarshalStrict"
                    | "encode"
                    | "decode"
            )
    }

    pub(super) fn generate_json_codec(
        &mut self,
        functions: &mut [hir::Function],
        copy: FunctionId,
        template: FunctionId,
        ty: TypeId,
        span: Span,
    ) {
        let name = functions[template.0 as usize].name.as_str();
        let decode = match name {
            "std/encoding/json.encode" => false,
            "std/encoding/json.decode" => true,
            _ => return,
        };
        if !self.validate_json_type(ty, decode, false, &self.name(ty), span, &mut HashSet::new()) {
            return;
        }
        let mut function = functions[copy.0 as usize].clone();
        function.native = false;
        let mut builder = CodecBuilder {
            checker: self,
            functions,
            function,
            template,
            span,
            decode,
        };
        let value = builder.local(builder.function.params[0]);
        let body = builder.codec(value);
        builder.function.body.stmts = body;
        if decode {
            let nil = builder.constant(Const::Nil, TypeStore::ERROR);
            builder
                .function
                .body
                .stmts
                .push(builder.statement(StmtKind::Return(vec![nil])));
        }
        functions[copy.0 as usize] = builder.function;
    }

    fn json_struct(&self, ty: TypeId, name: &str) -> bool {
        let Some(id) = self.types.struct_id(ty) else {
            return false;
        };
        let origin = self.types.struct_origin(id);
        self.res.structs[origin.0 as usize].name.text == name
            && self.res.packages[self.res.struct_package[origin.0 as usize]].path
                == "std/encoding/json"
    }

    fn validate_json_type(
        &mut self,
        ty: TypeId,
        decode: bool,
        fresh: bool,
        path: &str,
        span: Span,
        seen: &mut HashSet<(TypeId, bool)>,
    ) -> bool {
        if !seen.insert((ty, fresh)) {
            return true;
        }
        if decode
            && fresh
            && matches!(
                self.types.kind(ty),
                TypeKind::Interface(_) | TypeKind::InterfaceView { .. }
            )
        {
            self.error(format!("cannot decode {path}: a fresh codec interface needs a concrete type or custom decoder"), span);
            return false;
        }
        let method = if decode { "DecodeJSON" } else { "EncodeJSON" };
        if let Some(id) = self.method_of(ty, method) {
            let declaration = self.res.functions[id.0 as usize];
            let signature = self.signatures[id.0 as usize].as_ref().unwrap();
            let receiver = declaration.receiver.as_ref().unwrap();
            let correct_receiver = receiver.mode
                == if decode {
                    ParamMode::Mut
                } else {
                    ParamMode::Borrow
                };
            let correct_argument = signature.params.len() == 2
                && self.json_struct(
                    signature.params[1],
                    if decode { "Decoder" } else { "Encoder" },
                )
                && declaration.params[0].mode == ParamMode::Mut;
            let correct_results = if decode {
                signature.results == [TypeStore::ERROR]
            } else {
                signature.results.is_empty()
            };
            if correct_receiver && correct_argument && correct_results && !declaration.is_async {
                return true;
            }
            self.error(
                format!(
                    "cannot {} {path}: `{method}` has an incompatible codec signature",
                    if decode { "decode" } else { "encode" }
                ),
                span,
            );
            return false;
        }
        if self.json_struct(ty, "Value") {
            return true;
        }
        let valid = match self.types.kind(ty) {
            TypeKind::Bool | TypeKind::Int(_) | TypeKind::Float(_) | TypeKind::String => true,
            TypeKind::Struct(id) => {
                let fields = self.struct_fields(id);
                if !self.json_struct(ty, "Optional") {
                    for policy in 0..3 {
                        let mut names = HashSet::new();
                        for (name, _, _) in &fields {
                            if name.starts_with(|c: char| c.is_ascii_uppercase())
                                && !names.insert(field_name(name, policy))
                            {
                                self.error(format!("cannot {} {path}: JSON field name collision under naming policy {policy}", if decode { "decode" } else { "encode" }), span);
                                return false;
                            }
                        }
                    }
                }
                let mut valid = true;
                for (name, field, _) in fields {
                    if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                        valid &= self.validate_json_type(
                            field.unwrap(),
                            decode,
                            fresh,
                            &format!("{path}.{name}"),
                            span,
                            seen,
                        );
                    }
                }
                valid
            }
            TypeKind::Array { element, .. } => self.validate_json_type(
                element,
                decode,
                decode || fresh,
                &format!("{path}[]"),
                span,
                seen,
            ),
            TypeKind::DynArray { element } => {
                self.validate_json_type(element, decode, true, &format!("{path}[]"), span, seen)
            }
            TypeKind::Slice { element, .. } if !decode => {
                self.validate_json_type(element, decode, fresh, &format!("{path}[]"), span, seen)
            }
            TypeKind::Map { key, value } => {
                if !matches!(self.types.kind(key), TypeKind::String | TypeKind::Int(_)) {
                    false
                } else {
                    self.validate_json_type(
                        value,
                        decode,
                        true,
                        &format!("{path}[value]"),
                        span,
                        seen,
                    )
                }
            }
            TypeKind::Interface(_) | TypeKind::InterfaceView { .. } => {
                let interface = self.types.interface_of(ty).unwrap();
                let methods = self.types.interface_methods(interface);
                methods.len() == 1
                    && methods[0].name == method
                    && methods[0].receiver
                        == if decode {
                            ParamMode::Mut
                        } else {
                            ParamMode::Borrow
                        }
                    && !methods[0].is_async
                    && methods[0].params.len() == 1
                    && methods[0].params[0].0 == ParamMode::Mut
                    && self.json_struct(
                        methods[0].params[0].1,
                        if decode { "Decoder" } else { "Encoder" },
                    )
                    && methods[0].results
                        == if decode {
                            vec![TypeStore::ERROR]
                        } else {
                            vec![]
                        }
            }
            _ => false,
        };
        if !valid {
            self.error(
                format!(
                    "cannot {} {path}: {} has no JSON form",
                    if decode { "decode" } else { "encode" },
                    self.name(ty)
                ),
                span,
            );
        }
        valid
    }
}

fn field_name(name: &str, policy: u8) -> String {
    if policy == 0 {
        return name.into();
    }
    let letters: Vec<char> = name.chars().collect();
    let mut result = String::new();
    let mut initial = true;
    for (i, &c) in letters.iter().enumerate() {
        let upper = c.is_uppercase();
        let before_lower = letters.get(i + 1).is_some_and(|c| c.is_lowercase());
        let after_lower = i > 0 && (letters[i - 1].is_lowercase() || letters[i - 1].is_numeric());
        if policy == 2 {
            if i > 0 && upper && (after_lower || before_lower) {
                result.push('_');
            }
            result.push(simple_lower(c));
        } else {
            if i > 0 && (!upper || before_lower) {
                initial = false;
            }
            if initial {
                result.push(simple_lower(c));
            } else {
                result.push(c);
            }
        }
    }
    result
}

fn simple_lower(c: char) -> char {
    let mut lower = c.to_lowercase();
    let first = lower.next().unwrap();
    if lower.next().is_some() { c } else { first }
}

struct CodecBuilder<'a, 'b> {
    checker: &'a mut Checker<'b>,
    functions: &'a [hir::Function],
    function: hir::Function,
    template: FunctionId,
    span: Span,
    decode: bool,
}

impl CodecBuilder<'_, '_> {
    fn statement(&self, kind: StmtKind) -> Stmt {
        Stmt {
            kind,
            span: self.span,
        }
    }
    fn block(&self, stmts: Vec<Stmt>) -> hir::Block {
        hir::Block {
            stmts,
            span: self.span,
        }
    }
    fn constant(&self, value: Const, ty: TypeId) -> Expr {
        typed(ExprKind::Const(value), ty, self.span)
    }
    fn integer(&self, value: i128) -> Expr {
        self.constant(Const::Int(value), TypeStore::INT)
    }
    fn string(&self, value: &str) -> Expr {
        self.constant(Const::String(value.into()), TypeStore::STRING)
    }
    fn local(&self, id: LocalId) -> Expr {
        typed(
            ExprKind::Local(id),
            self.function.locals[id.0 as usize].ty,
            self.span,
        )
    }
    fn new_local(&mut self, ty: TypeId, kind: LocalKind) -> LocalId {
        let id = LocalId(self.function.locals.len() as u32);
        self.function.locals.push(hir::Local {
            name: format!("json{}", id.0),
            ty,
            kind,
            span: self.span,
        });
        id
    }
    fn bind(&mut self, value: Expr) -> (Expr, Stmt) {
        let id = self.new_local(value.ty(), LocalKind::Var);
        (
            self.local(id),
            self.statement(StmtKind::Let {
                targets: vec![Some(id)],
                value,
            }),
        )
    }
    fn call(&self, name: &str, args: Vec<Expr>) -> Expr {
        let index = self
            .functions
            .iter()
            .position(|f| f.name == name)
            .unwrap_or_else(|| panic!("missing JSON helper {name}"));
        Expr {
            kind: ExprKind::Call {
                function: FunctionId(index as u32),
                args,
            },
            types: self.functions[index].results.clone(),
            span: self.span,
        }
    }
    fn helper(&self, name: &str, args: Vec<Expr>) -> Expr {
        self.call(&format!("std/encoding/json.{name}"), args)
    }
    fn stream(&self) -> Expr {
        self.local(self.function.params[1])
    }
    fn naming(&self) -> Expr {
        self.local(self.function.params[2])
    }
    fn method(&self, name: &str, mut args: Vec<Expr>) -> Expr {
        args.insert(0, self.stream());
        self.helper(
            &format!(
                "{}.{}",
                if self.decode { "Decoder" } else { "Encoder" },
                name
            ),
            args,
        )
    }
    fn attempt(&self, call: Expr) -> Expr {
        let mut types = call.types.clone();
        types.pop();
        Expr {
            kind: ExprKind::Try(Box::new(call)),
            types,
            span: self.span,
        }
    }
    fn effect(&self, expr: Expr) -> Stmt {
        self.statement(StmtKind::Expr(expr))
    }
    fn stream_effect(&self, name: &str, args: Vec<Expr>) -> Stmt {
        let call = self.method(name, args);
        self.effect(if self.decode {
            self.attempt(call)
        } else {
            call
        })
    }
    fn nested(&self, value: Expr) -> Stmt {
        let call = Expr {
            kind: ExprKind::CallGeneric {
                function: self.template,
                type_args: vec![value.ty()],
                args: vec![value, self.stream(), self.naming()],
            },
            types: if self.decode {
                vec![TypeStore::ERROR]
            } else {
                vec![]
            },
            span: self.span,
        };
        self.effect(if self.decode {
            self.attempt(call)
        } else {
            call
        })
    }
    fn field(&self, value: Expr, index: usize) -> Expr {
        let id = self.checker.types.struct_id(value.ty()).unwrap();
        let ty = self.checker.struct_fields(id)[index].1.unwrap();
        typed(
            ExprKind::Field {
                base: Box::new(value),
                field: FieldId(index as u32),
            },
            ty,
            self.span,
        )
    }
    fn convert(&self, value: Expr, ty: TypeId) -> Expr {
        if value.ty() == ty {
            value
        } else {
            typed(ExprKind::Convert(Box::new(value)), ty, self.span)
        }
    }
    fn binary(&self, op: BinaryOp, lhs: Expr, rhs: Expr, ty: TypeId) -> Expr {
        typed(
            ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty,
            self.span,
        )
    }
    fn equals(&self, lhs: Expr, rhs: Expr) -> Expr {
        self.binary(BinaryOp::Eq, lhs, rhs, TypeStore::BOOL)
    }
    fn place(&self, value: &Expr) -> hir::Place {
        match &value.kind {
            ExprKind::Local(root) => hir::Place {
                root: *root,
                projections: vec![],
                ty: value.ty(),
                span: self.span,
            },
            ExprKind::Field { base, field } => {
                let mut p = self.place(base);
                p.projections.push(hir::Projection::Field(*field));
                p.ty = value.ty();
                p
            }
            ExprKind::Index { base, index } => {
                let mut p = self.place(base);
                p.projections.push(hir::Projection::Index(index.clone()));
                p.ty = value.ty();
                p
            }
            _ => unreachable!(),
        }
    }
    fn assign(&self, target: Expr, value: Expr) -> Stmt {
        self.statement(StmtKind::Assign {
            targets: vec![Some(self.place(&target))],
            values: vec![value],
        })
    }
    fn conditional(&self, condition: Expr, yes: Vec<Stmt>, no: Vec<Stmt>) -> Stmt {
        self.statement(StmtKind::If {
            condition,
            then_block: self.block(yes),
            else_block: (!no.is_empty()).then(|| self.block(no)),
        })
    }
    fn bind_zero(&mut self, ty: TypeId) -> (Expr, Stmt) {
        let array_type = self.checker.types.dyn_array_type(ty);
        let empty = typed(
            ExprKind::ArrayLit {
                element: ty,
                elements: vec![],
            },
            array_type,
            self.span,
        );
        let pop = Expr {
            kind: ExprKind::ArrayPop(Box::new(empty)),
            types: vec![TypeStore::BOOL, ty],
            span: self.span,
        };
        let local = self.new_local(ty, LocalKind::Var);
        (
            self.local(local),
            self.statement(StmtKind::Let {
                targets: vec![None, Some(local)],
                value: pop,
            }),
        )
    }
    fn zero(&mut self, ty: TypeId) -> Expr {
        let kind = match self.checker.types.kind(ty) {
            TypeKind::Bool => ExprKind::Const(Const::Bool(false)),
            TypeKind::Int(_) => ExprKind::Const(Const::Int(0)),
            TypeKind::Float(_) => ExprKind::Const(Const::Float(0.0)),
            TypeKind::String => ExprKind::Const(Const::String(String::new())),
            TypeKind::Error => ExprKind::Const(Const::Nil),
            TypeKind::Rune => ExprKind::Const(Const::Rune('\0')),
            TypeKind::Struct(id) => {
                let fields = self
                    .checker
                    .struct_fields(id)
                    .into_iter()
                    .enumerate()
                    .map(|(index, (_, ty, _))| (FieldId(index as u32), self.zero(ty.unwrap())))
                    .collect();
                ExprKind::StructLit { strukt: id, fields }
            }
            TypeKind::DynArray { element } => ExprKind::ArrayLit {
                element,
                elements: vec![],
            },
            TypeKind::Array { element, size } => ExprKind::ArrayLit {
                element,
                elements: (0..size).map(|_| self.zero(element)).collect(),
            },
            TypeKind::Map { key, value } => ExprKind::MapLit {
                key,
                value,
                entries: vec![],
            },
            _ => unreachable!("validated JSON zero type"),
        };
        typed(kind, ty, self.span)
    }
    fn codec(&mut self, value: Expr) -> Vec<Stmt> {
        let ty = value.ty();
        let method = if self.decode {
            "DecodeJSON"
        } else {
            "EncodeJSON"
        };
        if let Some(id) = self.checker.method_of(ty, method) {
            let args = vec![value, self.stream()];
            let type_args = self
                .checker
                .types
                .struct_id(ty)
                .and_then(|s| self.checker.types.instance_of(s))
                .map(|(_, a)| a.to_vec())
                .unwrap_or_default();
            let kind = if type_args.is_empty() {
                ExprKind::Call { function: id, args }
            } else {
                ExprKind::CallGeneric {
                    function: id,
                    type_args,
                    args,
                }
            };
            let call = Expr {
                kind,
                types: if self.decode {
                    vec![TypeStore::ERROR]
                } else {
                    vec![]
                },
                span: self.span,
            };
            return vec![self.effect(if self.decode {
                self.attempt(call)
            } else {
                call
            })];
        }
        if self.checker.json_struct(ty, "Value") {
            return if self.decode {
                vec![self.assign(value, self.attempt(self.method("Value", vec![])))]
            } else {
                vec![self.stream_effect("Value", vec![value])]
            };
        }
        if self.checker.json_struct(ty, "Optional") {
            let set = self.field(value.clone(), 1);
            let inner = self.field(value, 0);
            return if self.decode {
                vec![self.conditional(
                    self.method("IsNull", vec![]),
                    vec![
                        self.stream_effect("Null", vec![]),
                        self.assign(
                            set.clone(),
                            self.constant(Const::Bool(false), TypeStore::BOOL),
                        ),
                    ],
                    vec![
                        self.nested(inner),
                        self.assign(set, self.constant(Const::Bool(true), TypeStore::BOOL)),
                    ],
                )]
            } else {
                vec![self.conditional(
                    set,
                    vec![self.nested(inner)],
                    vec![self.stream_effect("Null", vec![])],
                )]
            };
        }
        match self.checker.types.kind(ty) {
            TypeKind::Bool | TypeKind::String | TypeKind::Int(_) | TypeKind::Float(_) => {
                self.primitive(value)
            }
            TypeKind::Struct(_) => self.struct_codec(value),
            TypeKind::Array { element, size } => self.array_codec(value, element, Some(size)),
            TypeKind::DynArray { element } | TypeKind::Slice { element, .. } => {
                self.array_codec(value, element, None)
            }
            TypeKind::Map {
                key,
                value: element,
            } => self.map_codec(value, key, element),
            TypeKind::Interface(_) | TypeKind::InterfaceView { .. } => {
                let call = Expr {
                    kind: ExprKind::InterfaceCall {
                        receiver: Box::new(value),
                        method: 0,
                        args: vec![self.stream()],
                    },
                    types: if self.decode {
                        vec![TypeStore::ERROR]
                    } else {
                        vec![]
                    },
                    span: self.span,
                };
                vec![self.effect(if self.decode {
                    self.attempt(call)
                } else {
                    call
                })]
            }
            _ => unreachable!("validated JSON type"),
        }
    }
    fn primitive(&self, value: Expr) -> Vec<Stmt> {
        let ty = value.ty();
        let (name, base, extra) = match self.checker.types.kind(ty) {
            TypeKind::Bool => ("Bool", TypeStore::BOOL, vec![]),
            TypeKind::String => ("String", TypeStore::STRING, vec![]),
            TypeKind::Int(integer) => (
                if integer.signed {
                    if self.decode { "signed" } else { "Int" }
                } else {
                    "unsigned"
                },
                if integer.signed {
                    TypeStore::INT64
                } else {
                    TypeStore::UINT64
                },
                if self.decode {
                    vec![self.integer(integer.bits.into())]
                } else {
                    vec![]
                },
            ),
            TypeKind::Float(float) => (
                "floatWidth",
                TypeStore::FLOAT64,
                vec![self.integer(float.bits.into())],
            ),
            _ => unreachable!(),
        };
        if self.decode {
            vec![self.assign(
                value,
                self.convert(self.attempt(self.method(name, extra)), ty),
            )]
        } else {
            let mut args = vec![self.convert(value, base)];
            args.extend(extra);
            vec![self.stream_effect(name, args)]
        }
    }
    fn field_keys(&mut self, names: &[String]) -> (Expr, Vec<Stmt>) {
        let ty = self.checker.types.dyn_array_type(TypeStore::STRING);
        let literals = |policy: u8| {
            typed(
                ExprKind::ArrayLit {
                    element: TypeStore::STRING,
                    elements: names
                        .iter()
                        .map(|name| self.string(&field_name(name, policy)))
                        .collect(),
                },
                ty,
                self.span,
            )
        };
        let policies = [literals(0), literals(1), literals(2)];
        let (keys, initialize) = self.bind(policies[0].clone());
        let mut statements = vec![initialize];
        for (policy, keys_value) in policies.iter().enumerate().skip(1) {
            statements.push(self.conditional(
                self.equals(
                    self.naming(),
                    self.constant(Const::Int(policy as i128), self.naming().ty()),
                ),
                vec![self.assign(keys.clone(), keys_value.clone())],
                vec![],
            ));
        }
        (keys, statements)
    }
    fn struct_codec(&mut self, value: Expr) -> Vec<Stmt> {
        let id = self.checker.types.struct_id(value.ty()).unwrap();
        let fields: Vec<_> = self
            .checker
            .struct_fields(id)
            .into_iter()
            .enumerate()
            .filter(|(_, (name, _, _))| name.starts_with(|c: char| c.is_ascii_uppercase()))
            .collect();
        let mut body = vec![self.stream_effect("BeginObject", vec![])];
        let names: Vec<String> = fields
            .iter()
            .map(|(_, (name, _, _))| name.clone())
            .collect();
        let (keys, configure_keys) = self.field_keys(&names);
        body.extend(configure_keys);
        if !self.decode {
            for (ordinal, (index, _)) in fields.into_iter().enumerate() {
                let field = self.field(value.clone(), index);
                let key = typed(
                    ExprKind::Index {
                        base: Box::new(keys.clone()),
                        index: Box::new(self.integer(ordinal as i128)),
                    },
                    TypeStore::STRING,
                    self.span,
                );
                let members = vec![
                    self.stream_effect("Field", vec![key]),
                    self.nested(field.clone()),
                ];
                if self.checker.json_struct(field.ty(), "Optional") {
                    body.push(self.conditional(self.field(field, 1), members, vec![]));
                } else {
                    body.extend(members);
                }
            }
        } else {
            for (index, _) in &fields {
                let field = self.field(value.clone(), *index);
                if self.checker.json_struct(field.ty(), "Optional") {
                    body.push(self.assign(
                        self.field(field, 1),
                        self.constant(Const::Bool(false), TypeStore::BOOL),
                    ));
                }
            }
            let slice_ty = self.checker.types.slice_type(TypeStore::STRING, false);
            let names_slice = typed(
                ExprKind::Slice {
                    base: Box::new(keys),
                    low: None,
                    high: None,
                    mutable: false,
                },
                slice_ty,
                self.span,
            );
            let (key, bind_key) = self.bind(self.attempt(self.method("Key", vec![])));
            let (index, bind_index) =
                self.bind(self.helper("fieldIndex", vec![key.clone(), names_slice]));
            let mut dispatch = vec![self.stream_effect("unknown", vec![key])];
            for (ordinal, (field, _)) in fields.iter().enumerate().rev() {
                dispatch = vec![self.conditional(
                    self.equals(index.clone(), self.integer(ordinal as i128)),
                    vec![self.nested(self.field(value.clone(), *field))],
                    dispatch,
                )];
            }
            let mut members = vec![bind_key, bind_index];
            members.extend(dispatch);
            body.push(self.statement(StmtKind::Loop {
                init: None,
                condition: Some(self.method("More", vec![])),
                update: None,
                body: self.block(members),
            }));
        }
        body.push(self.stream_effect("EndObject", vec![]));
        body
    }
    fn array_codec(&mut self, value: Expr, element: TypeId, size: Option<u32>) -> Vec<Stmt> {
        let mut body = vec![self.stream_effect("BeginArray", vec![])];
        if !self.decode {
            let item = self.new_local(element, LocalKind::Item);
            body.push(self.statement(StmtKind::ForEach {
                key: None,
                item: Some(item),
                collection: value,
                body: self.block(vec![self.nested(self.local(item))]),
            }));
        } else if let Some(size) = size {
            let (index, initialize) = self.bind(self.integer(0));
            body.push(initialize);
            let target = typed(
                ExprKind::Index {
                    base: Box::new(value),
                    index: Box::new(index.clone()),
                },
                element,
                self.span,
            );
            let overflow = self.binary(
                BinaryOp::GtEq,
                index.clone(),
                self.integer(size.into()),
                TypeStore::BOOL,
            );
            let failure = self.method("fail", vec![self.string("too many fixed-array elements")]);
            let (item, initialize) = self.bind_zero(element);
            let members = vec![
                self.conditional(
                    overflow,
                    vec![self.statement(StmtKind::Return(vec![failure]))],
                    vec![],
                ),
                initialize,
                self.nested(item.clone()),
                self.assign(target, item),
                self.assign(
                    index.clone(),
                    self.binary(
                        BinaryOp::Add,
                        index.clone(),
                        self.integer(1),
                        TypeStore::INT,
                    ),
                ),
            ];
            body.push(self.statement(StmtKind::Loop {
                init: None,
                condition: Some(self.method("More", vec![])),
                update: None,
                body: self.block(members),
            }));
            body.push(self.conditional(
                self.binary(
                    BinaryOp::NotEq,
                    index,
                    self.integer(size.into()),
                    TypeStore::BOOL,
                ),
                vec![self.statement(StmtKind::Return(vec![
                    self.method("fail", vec![self.string("too few fixed-array elements")]),
                ]))],
                vec![],
            ));
        } else {
            let zero = self.zero(value.ty());
            body.push(self.assign(value.clone(), zero));
            let (item, initialize) = self.bind_zero(element);
            let push = Expr {
                kind: ExprKind::ArrayPush {
                    array: Box::new(value),
                    value: Box::new(item.clone()),
                },
                types: vec![],
                span: self.span,
            };
            let members = vec![initialize, self.nested(item), self.effect(push)];
            body.push(self.statement(StmtKind::Loop {
                init: None,
                condition: Some(self.method("More", vec![])),
                update: None,
                body: self.block(members),
            }));
        }
        body.push(self.stream_effect("EndArray", vec![]));
        body
    }
    fn key_text(&self, key: Expr) -> Expr {
        match self.checker.types.kind(key.ty()) {
            TypeKind::String => self.convert(key, TypeStore::STRING),
            TypeKind::Int(integer) if integer.signed => {
                self.call("std/strconv.Itoa", vec![self.convert(key, TypeStore::INT)])
            }
            TypeKind::Int(_) => {
                self.helper("unsignedText", vec![self.convert(key, TypeStore::UINT64)])
            }
            _ => unreachable!(),
        }
    }
    fn map_codec(&mut self, value: Expr, key_type: TypeId, element: TypeId) -> Vec<Stmt> {
        let mut body = vec![self.stream_effect("BeginObject", vec![])];
        if self.decode {
            let (key, bind_key) = self.bind(self.attempt(self.method("Key", vec![])));
            let converted = match self.checker.types.kind(key_type) {
                TypeKind::String => self.convert(key, key_type),
                TypeKind::Int(integer) => self.convert(
                    self.attempt(self.method(
                        if integer.signed {
                            "signedKey"
                        } else {
                            "unsignedKey"
                        },
                        vec![key, self.integer(integer.bits.into())],
                    )),
                    key_type,
                ),
                _ => unreachable!(),
            };
            let (key, convert_key) = self.bind(converted);
            let (item, initialize) = self.bind_zero(element);
            let members = vec![
                bind_key,
                convert_key,
                initialize,
                self.nested(item.clone()),
                self.statement(StmtKind::MapAssign {
                    map: self.place(&value),
                    key,
                    value: item,
                }),
            ];
            body.push(self.statement(StmtKind::Loop {
                init: None,
                condition: Some(self.method("More", vec![])),
                update: None,
                body: self.block(members),
            }));
        } else {
            let names_ty = self.checker.types.dyn_array_type(TypeStore::STRING);
            let zero = self.zero(names_ty);
            let (names, initialize) = self.bind(zero);
            body.push(initialize);
            let key = self.new_local(key_type, LocalKind::Let);
            let append = Expr {
                kind: ExprKind::ArrayPush {
                    array: Box::new(names.clone()),
                    value: Box::new(self.key_text(self.local(key))),
                },
                types: vec![],
                span: self.span,
            };
            body.push(self.statement(StmtKind::ForEach {
                key: Some(key),
                item: None,
                collection: value.clone(),
                body: self.block(vec![self.effect(append)]),
            }));
            let (sorted, sort) = self.bind(self.helper("sortedNames", vec![names]));
            body.push(sort);
            let text = self.new_local(TypeStore::STRING, LocalKind::Item);
            let entry_key = self.new_local(key_type, LocalKind::Let);
            let entry = self.new_local(element, LocalKind::Item);
            let matching = self.conditional(
                self.equals(self.key_text(self.local(entry_key)), self.local(text)),
                vec![
                    self.stream_effect("Field", vec![self.local(text)]),
                    self.nested(self.local(entry)),
                    self.statement(StmtKind::Break),
                ],
                vec![],
            );
            let find = self.statement(StmtKind::ForEach {
                key: Some(entry_key),
                item: Some(entry),
                collection: value,
                body: self.block(vec![matching]),
            });
            body.push(self.statement(StmtKind::ForEach {
                key: None,
                item: Some(text),
                collection: sorted,
                body: self.block(vec![find]),
            }));
        }
        body.push(self.stream_effect("EndObject", vec![]));
        body
    }
}
