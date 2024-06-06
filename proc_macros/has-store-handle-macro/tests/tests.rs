use has_store_handle_macro::has_store_handle;

struct StoreHandle;

trait HasStoreHandle {
    fn store_handle(&self) -> &StoreHandle;
}

#[has_store_handle]
struct TestStruct<T: Clone> where T: Clone {
    #[cfg(test)]
    field1: i32,
    field2: String,
    field3: T
}

#[test]
fn it_works() {
    let v = TestStruct {
        field1: 5,
        field2: "hello".to_string(),
        field3: "winning".to_string(),
        store_handle: StoreHandle
    };
    
    assert_eq!(v.field3, "winning");
}