fn main() {
    let value = String::from("hello");
    drop(value);
    println!("{}", value);
}
