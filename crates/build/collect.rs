use clap::Clap;
use duct::cmd;

#[derive(Clap)]
pub struct Args {
	#[clap(multiple_values = true)]
	machines: Vec<String>,
}

pub fn run(args: Args) {
	for machine in args.machines {
		cmd!(
			"rsync",
			"--archive",
			"--compress",
			"--delete",
			"--progress",
			format!("{}:tangram/dist/*", machine),
			"dist",
		)
		.run()
		.unwrap();
	}
}
