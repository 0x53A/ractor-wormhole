use ractor_wormhole::serialization::ContextTransmaterializable;
use ractor_wormhole_derive::WormholeTransmaterializable;

// Structs
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct UnitStruct;

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct EmptyNamedStruct {}

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
#[allow(unreachable_code)]
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

// todo: unnamed structs in enum cases aren't supported yet
// #[derive(Debug, Clone, WormholeTransmaterializable)]
pub enum StructEnum {
    X { a: u32 },
}
