use crate::AppArgs;
use rsa::PublicKey;
use sha2::Digest;
use std::path::{Path, PathBuf};
use tangram_error::{err, Result};
use url::Url;

#[derive(Clone, serde::Deserialize)]
struct AppConfig {
	auth: Option<AuthConfig>,
	cookie_domain: Option<String>,
	database: Option<DatabaseConfig>,
	host: Option<std::net::IpAddr>,
	license: Option<PathBuf>,
	port: Option<u16>,
	smtp: Option<SmtpConfig>,
	storage: Option<StorageConfig>,
	url: Option<String>,
}

#[derive(Clone, serde::Deserialize)]
struct AuthConfig {
	enable: bool,
}

#[derive(Clone, serde::Deserialize)]
struct DatabaseConfig {
	max_connections: Option<u32>,
	url: Url,
}

#[derive(Clone, serde::Deserialize)]
struct SmtpConfig {
	host: String,
	password: String,
	username: String,
}

#[derive(Clone, serde::Deserialize)]
#[serde(tag = "type")]
enum StorageConfig {
	#[serde(rename = "local")]
	Local(LocalStorageConfig),
	#[serde(rename = "s3")]
	S3(S3StorageConfig),
}

#[derive(Clone, serde::Deserialize)]
struct LocalStorageConfig {
	path: PathBuf,
}

#[derive(Clone, serde::Deserialize)]
struct S3StorageConfig {
	pub access_key: String,
	pub secret_key: String,
	pub endpoint: String,
	pub bucket: String,
	pub region: String,
	pub cache_path: Option<PathBuf>,
}

#[cfg(feature = "app")]
pub fn app(args: AppArgs) -> Result<()> {
	let config: Option<AppConfig> = if let Some(config_path) = args.config {
		let config = std::fs::read(config_path)?;
		Some(serde_json::from_slice(&config)?)
	} else {
		None
	};
	let auth = config
		.as_ref()
		.and_then(|c| c.auth.as_ref())
		.and_then(|auth| {
			if auth.enable {
				Some(tangram_app::options::AuthOptions {})
			} else {
				None
			}
		});
	let cookie_domain = config.as_ref().and_then(|c| c.cookie_domain.clone());
	let storage = if let Some(storage) = config.as_ref().and_then(|c| c.storage.as_ref()) {
		match storage {
			StorageConfig::Local(storage) => tangram_app::options::StorageOptions::Local(
				tangram_app::options::LocalStorageOptions {
					path: storage.path.clone(),
				},
			),
			StorageConfig::S3(storage) => {
				let cache_path = storage
					.cache_path
					.clone()
					.unwrap_or_else(|| cache_path().unwrap());
				tangram_app::options::StorageOptions::S3(tangram_app::options::S3StorageOptions {
					access_key: storage.access_key.clone(),
					secret_key: storage.secret_key.clone(),
					endpoint: storage.endpoint.clone(),
					bucket: storage.bucket.clone(),
					region: storage.region.clone(),
					cache_path,
				})
			}
		}
	} else {
		tangram_app::options::StorageOptions::Local(tangram_app::options::LocalStorageOptions {
			path: data_path()?.join("data"),
		})
	};
	let database = config
		.as_ref()
		.and_then(|c| c.database.as_ref())
		.map(|database| tangram_app::options::DatabaseOptions {
			max_connections: database.max_connections,
			url: database.url.clone(),
		})
		.unwrap_or_else(|| tangram_app::options::DatabaseOptions {
			max_connections: None,
			url: default_database_url(),
		});
	let host_from_env = if let Ok(host) = std::env::var("HOST") {
		Some(host.parse()?)
	} else {
		None
	};
	let host_from_config = config.as_ref().and_then(|c| c.host);
	let host = host_from_env
		.or(host_from_config)
		.unwrap_or_else(|| "0.0.0.0".parse().unwrap());
	let port_from_env = if let Ok(port) = std::env::var("PORT") {
		Some(port.parse()?)
	} else {
		None
	};
	let port_from_config = config.as_ref().and_then(|c| c.port);
	let port = port_from_env.or(port_from_config).unwrap_or(8080);
	// Verify the license if one was provided.
	let license_verified: Option<bool> =
		if let Some(license_file_path) = config.as_ref().and_then(|c| c.license.clone()) {
			Some(verify_license(&license_file_path)?)
		} else {
			None
		};
	// Require a verified license if auth is enabled.
	if auth.is_some() {
		match license_verified {
			#[cfg(debug_assertions)]
			None => {}
			#[cfg(not(debug_assertions))]
			None => return Err(err!("a license is required to enable authentication")),
			Some(false) => return Err(err!("failed to verify license")),
			Some(true) => {}
		}
	}
	let smtp = if let Some(smtp) = config.as_ref().and_then(|c| c.smtp.clone()) {
		Some(tangram_app::options::SmtpOptions {
			host: smtp.host,
			username: smtp.username,
			password: smtp.password,
		})
	} else {
		None
	};
	let url = if let Some(url) = config.as_ref().and_then(|c| c.url.clone()) {
		Some(url.parse()?)
	} else {
		None
	};
	let options = tangram_app::options::Options {
		auth,
		cookie_domain,
		database,
		host,
		port,
		smtp,
		storage,
		url,
	};
	tangram_app::run(options)
}

/// Retrieve the user cache directory using the `dirs` crate.
#[cfg(feature = "app")]
fn cache_path() -> Result<PathBuf> {
	let cache_dir = dirs::cache_dir().ok_or_else(|| err!("failed to find user cache directory"))?;
	let tangram_cache_dir = cache_dir.join("tangram");
	std::fs::create_dir_all(&tangram_cache_dir).map_err(|_| {
		err!(
			"failed to create tangram cache directory in {}",
			tangram_cache_dir.display()
		)
	})?;
	Ok(tangram_cache_dir)
}

/// Retrieve the user data directory using the `dirs` crate.
#[cfg(feature = "app")]
fn data_path() -> Result<PathBuf> {
	let data_dir = dirs::data_dir().ok_or_else(|| err!("failed to find user data directory"))?;
	let tangram_data_dir = data_dir.join("tangram");
	std::fs::create_dir_all(&tangram_data_dir).map_err(|_| {
		err!(
			"failed to create tangram data directory in {}",
			tangram_data_dir.display()
		)
	})?;
	Ok(tangram_data_dir)
}

/// Retrieve the default database url, which is a sqlite database in the user data directory.
#[cfg(feature = "app")]
pub fn default_database_url() -> Url {
	let tangram_database_path = data_path().unwrap().join("db").join("tangram.db");
	std::fs::create_dir_all(tangram_database_path.parent().unwrap()).unwrap();
	let url = format!(
		"sqlite:{}",
		tangram_database_path.to_str().unwrap().to_owned()
	);
	Url::parse(&url).unwrap()
}

pub fn verify_license(license_file_path: &Path) -> Result<bool> {
	let tangram_license_public_key: &str = "
-----BEGIN RSA PUBLIC KEY-----
MIIBCgKCAQEAq+JphywG8wCe6cX+bx4xKH8xphMhaI5BgYefQHUXwp8xavoor6Fy
B54yZba/pkfTnao+P9BvPT0PlSJ1L9aGzq45lcQCcaT+ZdPC5qUogTrKu4eB2qSj
yTt5pGnPsna+/7yh2sDhC/SHMvTPKt4oHgobWYkH3/039Rj7z5X2WGq69gJzSknX
/lraNlVUqCWi3yCnMP9QOV5Tou5gQi4nxlfEJO3razrif5jHw1NufQ+xpx1GCpN9
WhFBU2R4GFZsxlEXV9g1Os1ZpyVuoOe9BnenuS57TixU9SC8kFUHAyAWRSiuLjoP
xAmGGm4wQ4FlMAt+Bj/K6rvdG3FJUu5ttQIDAQAB
-----END RSA PUBLIC KEY-----
";
	let tangram_license_public_key = tangram_license_public_key
		.lines()
		.skip(1)
		.filter(|line| !line.starts_with('-'))
		.fold(String::new(), |mut data, line| {
			data.push_str(&line);
			data
		});
	let tangram_license_public_key = base64::decode(tangram_license_public_key).unwrap();
	let tangram_license_public_key =
		rsa::RSAPublicKey::from_pkcs1(&tangram_license_public_key).unwrap();
	let license_data = std::fs::read(license_file_path)?;
	let mut sections = license_data.split(|byte| *byte == b':');
	let license_data = sections.next().ok_or_else(|| err!("invalid license"))?;
	let license_data = base64::decode(&license_data)?;
	let signature = sections.next().ok_or_else(|| err!("invalid license"))?;
	let signature = base64::decode(&signature)?;
	let mut digest = sha2::Sha256::new();
	digest.update(&license_data);
	let digest = digest.finalize();
	tangram_license_public_key.verify(
		rsa::PaddingScheme::new_pkcs1v15_sign(None),
		&digest,
		&signature,
	)?;
	Ok(true)
}
