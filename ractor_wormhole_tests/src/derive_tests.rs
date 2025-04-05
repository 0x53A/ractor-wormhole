use ractor_wormhole::serialization::ContextSerializable;
use ractor_wormhole_derive::WormholeSerializable;

// Structs
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, WormholeSerializable)]
pub struct UnitStruct;

#[derive(Debug, Clone, WormholeSerializable)]
pub struct EmptyNamedStruct {}

#[derive(Debug, Clone, WormholeSerializable)]
pub struct NamedStruct {
    pub a: u32,
    pub b: f32,
}

#[derive(Debug, Clone, WormholeSerializable)]
pub struct GenericNamedStruct<T1: ContextSerializable + Send, T2: ContextSerializable + Send> {
    pub a: T1,
    pub b: T2,
}

#[derive(Debug, Clone, WormholeSerializable)]
pub struct GenericNamedStructWithWhereClause<T1, T2>
where
    T1: ContextSerializable + Send,
    T2: ContextSerializable + Send,
{
    pub a: T1,
    pub b: T2,
}

#[derive(Debug, Clone, WormholeSerializable)]
pub struct NamedStr {
    pub a: (i32, u32),
    pub b: bool,
}

#[derive(Debug, Clone, WormholeSerializable)]
pub struct EmptyTupleStruct();

#[derive(Debug, Clone, WormholeSerializable)]
pub struct SingleTupleStruct(u32);

#[derive(Debug, Clone, WormholeSerializable)]
pub struct GenericSingleTupleStruct<T: ContextSerializable + Send>(T);

#[derive(Debug, Clone, WormholeSerializable)]
pub struct GenericSingleTupleStructWithWhereClause<T>(T)
where
    T: ContextSerializable + Send;

#[derive(Debug, Clone, WormholeSerializable)]
pub struct TwoTupleStruct(f32, f64);

// Enums
// -----------------------------------------------------------------------------

// note: this shows a 'unreachable' warning, todo: fix in 'WormholeSerializable'
#[allow(unreachable_code)]
#[derive(Debug, Clone, WormholeSerializable)]
pub enum EmptyEnum {}

#[derive(Debug, Clone, WormholeSerializable)]
pub enum SingleCaseNoDataEnum {
    A,
}

#[derive(Debug, Clone, WormholeSerializable)]
pub enum TwoCaseNoDataEnum {
    A,
    B,
}

#[derive(Debug, Clone, WormholeSerializable)]
pub enum SingleCaseIntEnum {
    B = 1,
}

// todo: unnamed structs in enum cases aren't supported yet
// #[derive(Debug, Clone, WormholeSerializable)]
pub enum StructEnum {
    X { a: u32 },
}
