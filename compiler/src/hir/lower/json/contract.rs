use std::collections::HashMap;

use crate::ast::ParamMode;
use crate::hir;
use crate::resolve::FunctionId;
use crate::types::{FuncSignature, StructId, TypeId, TypeStore};

use super::super::Checker;
use super::super::signature::function_mismatch;

pub(super) struct CodecContract {
    helpers: HashMap<String, FunctionId>,
    decode: bool,
    stream: TypeId,
    naming: TypeId,
    errors: Vec<TypeId>,
}

impl CodecContract {
    pub(super) fn resolve(
        functions: &[hir::Function],
        checker: &mut Checker<'_>,
        decode: bool,
    ) -> Result<Self, String> {
        use ParamMode::{Borrow, Mut, Own};
        let strukt = |name: &str| {
            (0..checker.types.struct_count())
                .map(|index| checker.types.struct_type(StructId(index as u32)))
                .find(|&ty| checker.json_struct(ty, name))
                .ok_or_else(|| format!("missing {name}"))
        };
        let encoder = strukt("Encoder")?;
        let decoder = strukt("Decoder")?;
        let tree = strukt("Value")?;
        let optional = strukt("Optional")?;
        let optional_id = checker
            .types
            .struct_id(optional)
            .ok_or("Optional needs Value T followed by Set bool")?;
        let fields = checker.struct_fields(optional_id);
        let optional_layout = match checker.types.template_params(optional_id) {
            Some(&[element]) => matches!(
                fields.as_slice(),
                [(first, Some(value), _), (second, Some(flag), _)]
                    if first == "Value"
                        && *value == element
                        && second == "Set"
                        && *flag == TypeStore::BOOL
            ),
            _ => false,
        };
        if !optional_layout {
            return Err("Optional needs Value T followed by Set bool".into());
        }
        let naming = checker
            .res
            .named
            .iter()
            .find(|(decl, package, _)| {
                decl.name.text == "Naming"
                    && checker.res.packages[*package].path == "std/encoding/json"
            })
            .map(|&(_, _, ty)| ty)
            .ok_or("missing Naming")?;
        if checker.types.kind(naming) != checker.types.kind(TypeStore::INT) {
            return Err("Naming must be an integer type".into());
        }
        let stream = if decode { decoder } else { encoder };
        let errors = if decode {
            vec![TypeStore::ERROR]
        } else {
            vec![]
        };
        let string_array = checker.types.dyn_array_type(TypeStore::STRING);
        let strings = checker.types.slice_type(TypeStore::STRING, false);
        let mut contract = Self {
            helpers: HashMap::new(),
            decode,
            stream,
            naming,
            errors: errors.clone(),
        };
        let mut require = |name: &str, params: Vec<(ParamMode, TypeId)>, results: Vec<TypeId>| {
            let Some((index, helper)) = functions.iter().enumerate().find(|(_, f)| f.name == name)
            else {
                return Err(format!("missing {name}"));
            };
            if let Some(detail) = function_mismatch(
                &checker.types,
                helper,
                &FuncSignature {
                    is_async: false,
                    params,
                    results,
                },
            ) {
                return Err(format!("{name}: {detail}"));
            }
            contract
                .helpers
                .insert(name.into(), FunctionId(index as u32));
            Ok(())
        };
        let prefix = if decode { "Decoder" } else { "Encoder" };
        for name in ["BeginObject", "EndObject", "BeginArray", "EndArray", "Null"] {
            require(
                &format!("std/encoding/json.{prefix}.{name}"),
                vec![(Mut, stream)],
                errors.clone(),
            )?;
        }
        for (name, ty) in [
            ("Bool", TypeStore::BOOL),
            ("String", TypeStore::STRING),
            ("Value", tree),
        ] {
            let mut params = vec![(Mut, stream)];
            let mut results = errors.clone();
            if decode {
                results.insert(0, ty);
            } else {
                params.push((Borrow, ty));
            }
            require(
                &format!("std/encoding/json.{prefix}.{name}"),
                params,
                results,
            )?;
        }
        if decode {
            for name in ["IsNull", "More"] {
                require(
                    &format!("std/encoding/json.Decoder.{name}"),
                    vec![(Mut, decoder)],
                    vec![TypeStore::BOOL],
                )?;
            }
            require(
                "std/encoding/json.Decoder.Key",
                vec![(Mut, decoder)],
                vec![TypeStore::STRING, TypeStore::ERROR],
            )?;
            for name in ["unknown", "fail"] {
                require(
                    &format!("std/encoding/json.Decoder.{name}"),
                    vec![(Mut, decoder), (Borrow, TypeStore::STRING)],
                    errors.clone(),
                )?;
            }
            for (name, ty) in [
                ("signed", TypeStore::INT64),
                ("unsigned", TypeStore::UINT64),
                ("floatWidth", TypeStore::FLOAT64),
            ] {
                require(
                    &format!("std/encoding/json.Decoder.{name}"),
                    vec![(Mut, decoder), (Borrow, TypeStore::INT)],
                    vec![ty, TypeStore::ERROR],
                )?;
            }
            for (name, ty) in [
                ("signedKey", TypeStore::INT64),
                ("unsignedKey", TypeStore::UINT64),
            ] {
                require(
                    &format!("std/encoding/json.Decoder.{name}"),
                    vec![
                        (Mut, decoder),
                        (Borrow, TypeStore::STRING),
                        (Borrow, TypeStore::INT),
                    ],
                    vec![ty, TypeStore::ERROR],
                )?;
            }
            require(
                "std/encoding/json.fieldIndex",
                vec![(Borrow, TypeStore::STRING), (Borrow, strings)],
                vec![TypeStore::INT],
            )?;
        } else {
            for (name, ty) in [
                ("Field", TypeStore::STRING),
                ("Int", TypeStore::INT),
                ("unsigned", TypeStore::UINT64),
            ] {
                require(
                    &format!("std/encoding/json.Encoder.{name}"),
                    vec![(Mut, encoder), (Borrow, ty)],
                    vec![],
                )?;
            }
            require(
                "std/encoding/json.Encoder.floatWidth",
                vec![
                    (Mut, encoder),
                    (Borrow, TypeStore::FLOAT64),
                    (Borrow, TypeStore::INT),
                ],
                vec![],
            )?;
            require(
                "std/encoding/json.unsignedText",
                vec![(Borrow, TypeStore::UINT64)],
                vec![TypeStore::STRING],
            )?;
            require(
                "std/encoding/json.sortedNames",
                vec![(Own, string_array)],
                vec![string_array],
            )?;
            require(
                "std/strconv.Itoa",
                vec![(Borrow, TypeStore::INT)],
                vec![TypeStore::STRING],
            )?;
        }
        Ok(contract)
    }

    pub(super) fn hook_mismatch(
        &self,
        types: &TypeStore,
        hook: &hir::Function,
        value: TypeId,
    ) -> Option<String> {
        use ParamMode::{Borrow, Mut};
        function_mismatch(
            types,
            hook,
            &FuncSignature {
                is_async: false,
                params: vec![
                    (if self.decode { Mut } else { Borrow }, value),
                    (Mut, self.stream),
                    (Borrow, self.naming),
                ],
                results: self.errors.clone(),
            },
        )
    }

    pub(super) fn helper(&self, name: &str) -> FunctionId {
        self.helpers[name]
    }
}
