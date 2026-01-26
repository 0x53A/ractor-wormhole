#![allow(unreachable_code)] // 'unreachable' warning, todo: fix in 'WormholeSerializable'

use ractor_wormhole::{
    WormholeTransmaterializable, transmaterialization::ContextTransmaterializable,
};

// Structs
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct UnitStruct;

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct EmptyNamedStruct {}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct SingleFieldNamedStruct {
    pub a: u32,
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct NamedStruct {
    pub a: u32,
    pub b: f32,
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct GenericNamedStruct<
    T1: ContextTransmaterializable + Send,
    T2: ContextTransmaterializable + Send,
> {
    pub a: T1,
    pub b: T2,
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct GenericNamedStructWithWhereClause<T1, T2>
where
    T1: ContextTransmaterializable + Send,
    T2: ContextTransmaterializable + Send,
{
    pub a: T1,
    pub b: T2,
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct NamedStr {
    pub a: (i32, u32),
    pub b: bool,
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct EmptyTupleStruct();

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct SingleTupleStruct(u32);

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct GenericSingleTupleStruct<T: ContextTransmaterializable + Send>(T);

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct GenericSingleTupleStructWithWhereClause<T>(T)
where
    T: ContextTransmaterializable + Send;

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct TwoTupleStruct(f32, f64);

// Enums
// -----------------------------------------------------------------------------

// note: this shows a 'unreachable' warning, todo: fix in 'WormholeSerializable'

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub enum EmptyEnum {}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub enum SingleCaseNoDataEnum {
    A,
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub enum TwoCaseNoDataEnum {
    A,
    B,
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub enum SingleCaseIntEnum {
    B = 1,
}

#[derive(Debug, Clone, PartialEq, WormholeTransmaterializable)]
pub enum SingleTupleVariantEnum {
    A(u32),
}

#[derive(Debug, Clone, PartialEq, WormholeTransmaterializable)]
pub enum EmptyVariantsEnum {
    EmptyTuple(),
    EmptyStruct {},
}

#[derive(Debug, Clone, PartialEq, WormholeTransmaterializable)]
pub enum StructEnum {
    X { a: u32 },
}

#[derive(Debug, Clone, PartialEq, WormholeTransmaterializable)]
pub enum MultiStructVariantEnum {
    First { x: u32 },
    Second { y: String, z: bool },
}

#[derive(Debug, Clone, PartialEq, WormholeTransmaterializable)]
pub enum MixedEnum {
    Unit,
    Tuple(u32, String),
    Struct { a: u32, b: String },
}

#[derive(Debug, Clone, PartialEq, WormholeTransmaterializable)]
pub enum GenericEnum<T: ContextTransmaterializable + Send> {
    Value(T),
    None,
}
