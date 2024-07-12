use config::Config;
use url::Url;
use matrix_sdk::Client;

#[tokio::main]
async fn main() {
    let config = Config::builder()
        .add_source(config::File::with_name("config.yaml"))
        .build()
        .unwrap();

    let hs = config.get::<String>("matrix.homeserver_url").expect("Homeserver url missing in config");
    let hs_url = Url::parse(&hs).expect("Invalid homeserver url");
    let username = config.get::<String>("matrix.username").expect("Username missing in config");
    let password = config.get::<String>("matrix.password").expect("Password missing in config");
    println!("Loging into {hs_url} as {username}...");

    let client = Client::new(hs_url)
        .await
        .expect("Failed to connect to server");

    let response = client
        .matrix_auth()
        .login_username(&username, &password)
        .initial_device_display_name("wipbot")
        .await
        .expect("Login failed");

    println!("Logged in successfully!.");

    println!("Logging out...");
    let response = client.matrix_auth().logout().await;
    match response {
        Ok(_) => println!("Successfully logged out!"),
        Err(e) => println!("Failed to log out: {e:?}"),
    }
}
