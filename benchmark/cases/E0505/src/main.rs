fn main() {
    let v = vec![1, 2, 3];
    let r = &v;
    let owned = v;
    println!("{}", r.len());
    println!("{}", owned.len());
}
