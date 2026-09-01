fn print_value<T: std::fmt::Display>(v: T) {
    println!("{}", v);
}
fn main() {
    let v = vec![1, 2, 3];
    print_value(v);
}
