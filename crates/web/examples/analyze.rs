//! Native reference for the WebAssembly build: prints the same JSON the
//! browser gets for `analyze` and `tracks` (tools/check-wasm.js compares them).
fn main() {
    let path = std::env::args().nth(1).expect("usage: analyze <file.wav>");
    let bytes = std::fs::read(path).expect("read wav");
    let sr = unsafe { steroid_web::ssb_load_wav(bytes.as_ptr(), bytes.len()) };
    assert!(sr > 0, "not a WAV file");
    for m in ["analyze", "tracks"] {
        let rc = unsafe { steroid_web::ssb_call(m.as_ptr(), m.len(), std::ptr::null(), 0) };
        let out = unsafe { std::slice::from_raw_parts(steroid_web::ssb_out_ptr(), steroid_web::ssb_out_len()) };
        assert_eq!(rc, 0, "{}", String::from_utf8_lossy(out));
        println!("{}", String::from_utf8_lossy(out));
    }
}
