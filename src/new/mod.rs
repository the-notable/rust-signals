use std::collections::BTreeMap;

struct Changeable<T>(T);

struct ChangeableMap<K, V>(Changeable<BTreeMap<K, V>>);

trait Watch {}