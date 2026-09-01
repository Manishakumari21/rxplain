fn push_value(v: &Vec<i32>) {
    v.push(1);
}

fn main() {
    let nums = vec![1, 2, 3];
    push_value(&nums);
}
