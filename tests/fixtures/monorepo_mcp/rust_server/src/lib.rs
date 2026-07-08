pub fn setup(builder: &mut ServerBuilder) {
    builder.tool("evaluate", |args| Ok(()));
}
