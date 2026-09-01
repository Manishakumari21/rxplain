fn main() {
    let mut x = 5;
    let r = &x;
    x = 10;
    println!("{}", r);
}