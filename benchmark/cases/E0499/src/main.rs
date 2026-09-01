fn main() {
    let mut value = 10;
    let first = &mut value;
    let second = &mut value;
    println!("{} {}", first, second);
}
