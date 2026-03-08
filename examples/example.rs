use yanet::Report;
use yanet::Result;
use yanet::ResultExt;

pub fn main() -> Result<()> {
    Err(Report::new("foo").wrap("bar")).wrap_err("baz")
}
