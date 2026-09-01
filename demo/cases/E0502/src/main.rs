fn main() {
    let mut value = 10;
    let reference = &value;
    let mutable_reference = &mut value;
    println!("{}", reference);
    *mutable_reference += 1;
}
