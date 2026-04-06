use stratakv::Store;

fn main() {
    let mut store = Store::new();

    println!("=== StrataKV Hello World ===\n");

    store.put(b"name".to_vec(), b"StrataKV".to_vec());
    store.put(b"version".to_vec(), b"0.1.0".to_vec());
    store.put(b"language".to_vec(), b"Rust".to_vec());

    println!("Inserted 3 key-value pairs.");
    println!("Store size: {}\n", store.len());

    for key in &[
        b"name".as_slice(),
        b"version".as_slice(),
        b"language".as_slice(),
    ] {
        let value = store.get(key).expect("key should exist");
        println!(
            "  {} = {}",
            std::str::from_utf8(key).unwrap(),
            std::str::from_utf8(value).unwrap()
        );
    }

    println!("\nOverwriting 'version' -> '0.2.0'");
    let prev = store.put(b"version".to_vec(), b"0.2.0".to_vec());
    println!(
        "  Previous value: {}",
        std::str::from_utf8(&prev.unwrap()).unwrap()
    );
    println!(
        "  New value:      {}",
        std::str::from_utf8(store.get(b"version").unwrap()).unwrap()
    );

    println!("\nDeleting 'language'");
    let removed = store.delete(b"language").unwrap();
    println!(
        "  Removed value: {}",
        std::str::from_utf8(&removed).unwrap()
    );
    println!("  Store size: {}", store.len());

    println!("\n=== Done! ===");
}
