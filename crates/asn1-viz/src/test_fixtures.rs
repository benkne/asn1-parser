//! Small `IrProgram` fixtures shared between tree-rendering and HTML-export
//! tests. Keeping them here avoids duplicating the setup between submodules.

use asn1_ir::{
    IrField, IrItem, IrModule, IrOptionality, IrProgram, IrStruct, IrStructMember, IrType,
    IrTypeDef,
};

pub(crate) fn program_with_reference_chain() -> IrProgram {
    let id = IrTypeDef {
        name: "Id".into(),
        doc: None,
        ty: IrType::Integer { named_numbers: vec![], constraints: vec![] },
    };
    let inner = IrTypeDef {
        name: "Inner".into(),
        doc: None,
        ty: IrType::Sequence(IrStruct {
            extensible: false,
            members: vec![IrStructMember::Field(IrField {
                doc: None,
                name: "id".into(),
                ty: IrType::Reference { module: Some("M".into()), name: "Id".into() },
                optionality: IrOptionality::Required,
                is_extension: false,
            })],
        }),
    };
    let outer = IrTypeDef {
        name: "Outer".into(),
        doc: None,
        ty: IrType::Sequence(IrStruct {
            extensible: false,
            members: vec![IrStructMember::Field(IrField {
                doc: None,
                name: "inner".into(),
                ty: IrType::Reference { module: Some("M".into()), name: "Inner".into() },
                optionality: IrOptionality::Required,
                is_extension: false,
            })],
        }),
    };
    IrProgram {
        modules: vec![IrModule {
            name: "M".into(),
            oid: None,
            imports: vec![],
            items: vec![IrItem::Type(id), IrItem::Type(inner), IrItem::Type(outer)],
        }],
    }
}

pub(crate) fn tiny_program() -> IrProgram {
    let point = IrTypeDef {
        name: "Point".into(),
        doc: Some("a 2-d point".into()),
        ty: IrType::Sequence(IrStruct {
            extensible: false,
            members: vec![
                IrStructMember::Field(IrField {
                    doc: None,
                    name: "x".into(),
                    ty: IrType::Integer { named_numbers: vec![], constraints: vec![] },
                    optionality: IrOptionality::Required,
                    is_extension: false,
                }),
                IrStructMember::Field(IrField {
                    doc: None,
                    name: "y".into(),
                    ty: IrType::Integer { named_numbers: vec![], constraints: vec![] },
                    optionality: IrOptionality::Optional,
                    is_extension: false,
                }),
            ],
        }),
    };
    IrProgram {
        modules: vec![IrModule {
            name: "Geo".into(),
            oid: None,
            imports: vec![],
            items: vec![IrItem::Type(point)],
        }],
    }
}

pub(crate) fn program_with_self_reference() -> IrProgram {
    // Node ::= SEQUENCE { child Node OPTIONAL }
    let node = IrTypeDef {
        name: "Node".into(),
        doc: None,
        ty: IrType::Sequence(IrStruct {
            extensible: false,
            members: vec![IrStructMember::Field(IrField {
                doc: None,
                name: "child".into(),
                ty: IrType::Reference { module: Some("M".into()), name: "Node".into() },
                optionality: IrOptionality::Optional,
                is_extension: false,
            })],
        }),
    };
    IrProgram {
        modules: vec![IrModule {
            name: "M".into(),
            oid: None,
            imports: vec![],
            items: vec![IrItem::Type(node)],
        }],
    }
}
