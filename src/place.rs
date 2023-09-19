use crate::cil_op::{CILOp, FieldDescriptor};
use rustc_middle::mir::{Place, PlaceElem};
use rustc_middle::ty::{IntTy, Ty, TyCtxt, TyKind};
fn slice_head<T>(slice: &[T]) -> (&T, &[T]) {
    assert!(!slice.is_empty());
    let last = &slice[slice.len() - 1];
    (last, &slice[..(slice.len() - 1)])
}
fn pointed_type(ty: Ty) -> Ty {
    if let TyKind::Ref(_region, inner, _mut) = ty.kind() {
        *inner
    } else {
        panic!("{ty:?} is not a pointer type!");
    }
}
fn body_ty_is_by_adress(last_ty: &Ty) -> bool {
    match *last_ty.kind() {
        TyKind::Int(_) => false,
        TyKind::Adt(_, _) => true,
        TyKind::Ref(_region, inner, _mut) => false,
        _ => todo!("TODO: body_ty_is_by_adress does not support type {last_ty:?}"),
    }
}
fn local_get(local: usize, method: &rustc_middle::mir::Body) -> CILOp {
    if local == 0 {
        CILOp::LDLoc(0)
    } else if local > method.arg_count {
        CILOp::LDLoc((local - method.arg_count) as u32)
    } else {
        CILOp::LDArg((local - 1) as u32)
    }
}
fn local_set(local: usize, method: &rustc_middle::mir::Body) -> CILOp {
    if local == 0 {
        CILOp::STLoc(0)
    } else if local > method.arg_count {
        CILOp::STLoc((local - method.arg_count) as u32)
    } else {
        CILOp::STArg((local - 1) as u32)
    }
}
fn local_adress(local: usize, method: &rustc_middle::mir::Body) -> CILOp {
    if local == 0 {
        CILOp::LDLocA(0)
    } else if local > method.arg_count {
        CILOp::LDLocA((local - method.arg_count) as u32)
    } else {
        CILOp::LDArgA((local - 1) as u32)
    }
}
fn local_body<'tcx>(local: usize, method: &rustc_middle::mir::Body<'tcx>) -> (CILOp, Ty<'tcx>) {
    let ty = method.local_decls[local.into()].ty;
    if body_ty_is_by_adress(&ty) {
        (local_adress(local, method), ty)
    } else {
        (local_get(local, method), ty)
    }
}
/// Returns the ops for getting the value of place.
pub fn place_get<'a>(
    place: &Place<'a>,
    ctx: TyCtxt<'a>,
    method: &rustc_middle::mir::Body<'a>,
) -> Vec<CILOp> {
    let mut ops = Vec::with_capacity(place.projection.len());
    if place.projection.is_empty() {
        ops.push(local_get(place.local.as_usize(), method));
        return ops;
    } else {
        let (op, mut ty) = local_body(place.local.as_usize(), method);
        ops.push(op);
        let (head, body) = slice_head(place.projection);
        for elem in body {
            println!("elem:{elem:?} ty:{ty:?}");
            let (curr_ty, curr_ops) = place_elem_body(elem, ty, ctx);
            ty = curr_ty;
            ops.extend(curr_ops);
        }
        ops.extend(place_elem_get(head, ty, ctx));
        ops
    }
}
fn place_elem_get<'a>(
    place_elem: &PlaceElem<'a>,
    curr_type: Ty<'a>,
    ctx: TyCtxt<'a>,
) -> Vec<CILOp> {
    match place_elem {
        PlaceElem::Deref => deref_op(curr_type),
        PlaceElem::Field(index, field_type) => {
            let field_name = field_name(curr_type, index.as_u32());
            let curr_type = crate::r#type::Type::from_ty(curr_type, ctx);
            let curr_type = if let crate::r#type::Type::DotnetType(dotnet_type) = curr_type {
                dotnet_type.as_ref().clone()
            } else {
                panic!();
            };
            let field_desc = FieldDescriptor::boxed(
                curr_type,
                crate::r#type::Type::from_ty(*field_type, ctx),
                field_name,
            );
            vec![CILOp::LDField(field_desc)]
        }
        _ => todo!("Can't handle porojection {place_elem:?} in get"),
    }
}
fn field_name(ty: Ty, idx: u32) -> crate::IString {
    match ty.kind() {
        TyKind::Adt(adt_def, subst) => {
            let field_def = adt_def
                .all_fields()
                .nth(idx as usize)
                .expect("Field index out of range.");
            field_def.name.to_string().into()
        }
        _ => todo!("Can't yet get fields of typr {ty:?}"),
    }
}
fn place_elem_body<'ctx>(
    place_elem: &PlaceElem<'ctx>,
    curr_type: Ty<'ctx>,
    tyctx: TyCtxt<'ctx>,
) -> (Ty<'ctx>, Vec<CILOp>) {
    match place_elem {
        PlaceElem::Deref => {
            let pointed = pointed_type(curr_type);
            if body_ty_is_by_adress(&pointed) {
                (pointed, vec![])
            } else {
                (pointed, deref_op(curr_type))
            }
        }
        PlaceElem::Field(index, field_type) => {
            let field_name = field_name(curr_type, index.as_u32());
            let curr_type = crate::r#type::Type::from_ty(curr_type, tyctx);
            let curr_type = if let crate::r#type::Type::DotnetType(dotnet_type) = curr_type {
                dotnet_type.as_ref().clone()
            } else {
                panic!();
            };
            let field_desc = FieldDescriptor::boxed(
                curr_type,
                crate::r#type::Type::from_ty(*field_type, tyctx),
                field_name,
            );
            if body_ty_is_by_adress(&field_type) {
                (*field_type, vec![CILOp::LDFieldAdress(field_desc)])
            } else {
                (*field_type, vec![CILOp::LDField(field_desc)])
            }
        }
        _ => todo!("Can't handle porojection {place_elem:?} in body"),
    }
}
fn deref_op(curr_type: Ty) -> Vec<CILOp> {
    match curr_type.kind() {
        TyKind::Int(int_ty) => match int_ty {
            IntTy::I8 => vec![CILOp::LDIndI8],
            _ => todo!("TODO: can't deref int type {int_ty:?} yet"),
        },
        _ => todo!("TODO: can't deref type {curr_type:?} yet"),
    }
}
/// Returns the ops for getting the value of place.
pub fn place_adress<'a>(
    place: &Place<'a>,
    ctx: TyCtxt<'a>,
    method: &rustc_middle::mir::Body<'a>,
) -> Vec<CILOp> {
    let mut ops = Vec::with_capacity(place.projection.len());
    if place.projection.is_empty() {
        ops.push(local_adress(place.local.as_usize(), method));
        return ops;
    } else {
        let (op, mut ty) = local_body(place.local.as_usize(), method);
        ops.push(op);
        todo!();
    }
}
pub(crate) fn place_set<'a>(
    place: &Place<'a>,
    ctx: TyCtxt<'a>,
    value_calc: Vec<CILOp>,
    method: &rustc_middle::mir::Body<'a>,
) -> Vec<CILOp> {
    let mut ops = Vec::with_capacity(place.projection.len());
    if place.projection.is_empty() {
        ops.extend(value_calc);
        ops.push(local_set(place.local.as_usize(), method));
        return ops;
    } else {
        let (op, mut ty) = local_body(place.local.as_usize(), method);
        ops.push(op);
        todo!();
    }
}