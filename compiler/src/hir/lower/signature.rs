use crate::ast::ParamMode;
use crate::hir;
use crate::resolve::LocalKind;
use crate::types::{FuncSignature, InterfaceMethod, TypeId, TypeStore};

pub(super) fn signature_mismatch(
    types: &TypeStore,
    actual: &FuncSignature,
    expected: &FuncSignature,
) -> Option<String> {
    if actual.is_async != expected.is_async {
        return Some(
            if expected.is_async {
                "must be async"
            } else {
                "must be synchronous"
            }
            .into(),
        );
    }
    if actual.params.len() != expected.params.len() {
        return Some(format!(
            "takes {} parameters, expected {}",
            actual.params.len(),
            expected.params.len()
        ));
    }
    for (index, (&found, &wanted)) in actual.params.iter().zip(&expected.params).enumerate() {
        if found == wanted {
            continue;
        }
        let found_text = parameter_text(types, found);
        let wanted_text = parameter_text(types, wanted);
        let position = index + 1;
        return Some(if found_text == wanted_text {
            format!("parameter {position} is a different type named `{wanted_text}`")
        } else {
            format!("parameter {position} is `{found_text}`, expected `{wanted_text}`")
        });
    }
    if actual.results != expected.results {
        return Some(format!(
            "returns `{}`, expected `{}`",
            results_text(types, &actual.results),
            results_text(types, &expected.results)
        ));
    }
    None
}

pub(super) fn function_mismatch(
    types: &TypeStore,
    function: &hir::Function,
    expected: &FuncSignature,
) -> Option<String> {
    if !function.type_params.is_empty() {
        return Some("must not declare type parameters".into());
    }
    if function.call_once {
        return Some("must be reusable".into());
    }
    let params = function
        .params
        .iter()
        .map(|id| {
            let local = &function.locals[id.0 as usize];
            let LocalKind::Param(mode) = local.kind else {
                unreachable!("a function parameter local has a parameter kind");
            };
            (mode, local.ty)
        })
        .collect();
    let actual = FuncSignature {
        is_async: function.is_async,
        params,
        results: function.results.clone(),
    };
    signature_mismatch(types, &actual, expected)
}

pub(super) fn method_mismatch(
    types: &TypeStore,
    method: &InterfaceMethod,
    expected: &FuncSignature,
) -> Option<String> {
    let actual = FuncSignature {
        is_async: method.is_async,
        params: method.params.clone(),
        results: method.results.clone(),
    };
    signature_mismatch(types, &actual, expected)
}

fn parameter_text(types: &TypeStore, (mode, ty): (ParamMode, TypeId)) -> String {
    let prefix = match mode {
        ParamMode::Mut => "mut ",
        ParamMode::Own => "own ",
        ParamMode::Borrow => "",
    };
    format!("{prefix}{}", types.display(ty))
}

fn results_text(types: &TypeStore, results: &[TypeId]) -> String {
    match results {
        [] => "nothing".into(),
        [only] => types.display(*only).to_string(),
        several => {
            let texts: Vec<String> = several
                .iter()
                .map(|&ty| types.display(ty).to_string())
                .collect();
            format!("({})", texts.join(", "))
        }
    }
}
