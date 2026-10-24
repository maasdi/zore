use std::collections::HashMap;

use crate::ast::ParamMode;
use crate::hir;
use crate::resolve::{FunctionId, LocalKind};
use crate::types::{StructId, TypeId, TypeStore};

use super::super::Checker;

pub(super) struct CodecContract {
    helpers: HashMap<String, FunctionId>,
}

pub(super) fn signature_matches(
    function: &hir::Function,
    params: &[(ParamMode, TypeId)],
    results: &[TypeId],
) -> bool {
    !function.is_async
        && !function.call_once
        && function.type_params.is_empty()
        && function.results == results
        && function.params.len() == params.len()
        && function.params.iter().zip(params).all(|(id, &(mode, ty))| {
            function
                .locals
                .get(id.0 as usize)
                .is_some_and(|local| local.kind == LocalKind::Param(mode) && local.ty == ty)
        })
}

impl CodecContract {
    pub(super) fn resolve(
        functions: &[hir::Function],
        checker: &mut Checker<'_>,
        function: &hir::Function,
        value: TypeId,
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
        let naming = function
            .params
            .get(2)
            .and_then(|id| function.locals.get(id.0 as usize))
            .map(|local| local.ty)
            .filter(|&ty| {
                checker.res.named.iter().any(|(decl, package, named)| {
                    *named == ty
                        && decl.name.text == "Naming"
                        && checker.res.packages[*package].path == "std/encoding/json"
                }) && checker.types.kind(ty) == checker.types.kind(TypeStore::INT)
            })
            .ok_or("codec Naming parameter")?;
        let stream = if decode { decoder } else { encoder };
        let errors = if decode {
            vec![TypeStore::ERROR]
        } else {
            vec![]
        };
        if !signature_matches(
            function,
            &[
                (if decode { Mut } else { Borrow }, value),
                (Mut, stream),
                (Borrow, naming),
            ],
            &errors,
        ) {
            return Err("encode/decode hook signature".into());
        }
        let string_array = checker.types.dyn_array_type(TypeStore::STRING);
        let strings = checker.types.slice_type(TypeStore::STRING, false);
        let mut contract = Self {
            helpers: HashMap::new(),
        };
        let mut require = |name: &str, params: Vec<(ParamMode, TypeId)>, results: Vec<TypeId>| {
            let Some((index, helper)) = functions.iter().enumerate().find(|(_, f)| f.name == name)
            else {
                return Err(format!("missing {name}"));
            };
            if !signature_matches(helper, &params, &results) {
                return Err(format!("{name} signature"));
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

    pub(super) fn helper(&self, name: &str) -> FunctionId {
        self.helpers[name]
    }
}
