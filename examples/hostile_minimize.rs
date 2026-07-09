fn fails(input: &[u8]) -> bool {
    let o1 = oxideav_ass::parse_script(input).serialise();
    let o2 = oxideav_ass::parse_script(&o1).serialise();
    o1 != o2
}
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut cur = std::fs::read(&path).unwrap();
    assert!(fails(&cur), "input does not fail");
    let mut chunk = cur.len() / 2;
    while chunk > 0 {
        let mut i = 0;
        while i + chunk <= cur.len() {
            let mut cand = cur.clone();
            cand.drain(i..i + chunk);
            if fails(&cand) {
                cur = cand;
            } else {
                i += chunk.max(1);
            }
        }
        chunk /= 2;
    }
    println!(
        "minimized {} bytes: {:?}",
        cur.len(),
        String::from_utf8_lossy(&cur)
    );
    let o1 = oxideav_ass::parse_script(&cur).serialise();
    let o2 = oxideav_ass::parse_script(&o1).serialise();
    println!("o1={:?}", String::from_utf8_lossy(&o1));
    println!("o2={:?}", String::from_utf8_lossy(&o2));
}
