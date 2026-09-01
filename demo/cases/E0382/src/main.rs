fn main() {
    let message = String::from("hello");
    let loud = message.to_uppercase();
    drop(message);
    println!("{} {}", loud, message);
}
