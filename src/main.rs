use config::Config;
use url::Url;
use matrix_sdk::{
    config::SyncSettings,
    matrix_auth::MatrixSession,
    Client, Room, RoomState,
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
    },
    ruma::events::room::member::StrippedRoomMemberEvent,
};
use tokio::fs;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::builder()
        .add_source(config::File::with_name("config.yaml"))
        .build()
        .unwrap();

    let hs = config.get::<String>("matrix.homeserver_url").expect("Homeserver url missing in config");
    let hs_url = Url::parse(&hs).expect("Invalid homeserver url");
    let username = config.get::<String>("matrix.username").expect("Username missing in config");
    let password = config.get::<String>("matrix.password").expect("Password missing in config");

    let data_dir = dirs::data_dir().expect("no data_dir directory found").join("matrix-wip-bot");
    let db_path = data_dir.join("db");
    let session_path = data_dir.join("session");

    println!("Data dir configured at {}", data_dir.to_str().unwrap_or_default());
    println!("Logging into {hs_url} as {username}...");

    let client = Client::builder()
        .homeserver_url(&hs_url)
        .sqlite_store(&db_path, None)
        .build()
        .await?;

    if session_path.exists() {
        println!("Restoring old login...");
        let serialized_session = fs::read_to_string(session_path).await?;
        let user_session: MatrixSession = serde_json::from_str(&serialized_session)?;
        client.restore_session(user_session).await?;
    } else {
        println!("Doing a fresh login...");

        let matrix_auth = client.matrix_auth();
        let login_response = matrix_auth
            .login_username(&username, &password)
            .initial_device_display_name("wip-bot")
            .await?;

        println!("Logged in as {}", login_response.device_id);

        let user_session = matrix_auth.session().expect("A logged-in client should have a session");
        let serialized_session = serde_json::to_string(&user_session)?;
        fs::write(session_path, serialized_session).await?;
    }

    // This one is possibly also for old state events handled before
    client.add_event_handler(handle_invites);

    // Sync once without message handler to not deal with old messages
    client.sync_once(SyncSettings::default()).await?;
    println!("Initial sync finished, start listening for events");

    client.add_event_handler(handle_message);

    // Client will re-use the previously stored sync token automatically
    client.sync(SyncSettings::default()).await?;

    Ok(())
}

// From https://github.com/matrix-org/matrix-rust-sdk/blob/main/examples/autojoin/src/main.rs
async fn handle_invites(
    room_member: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
) {
    if room_member.state_key != client.user_id().unwrap() {
        return;
    }

    tokio::spawn(async move {
        println!("Autojoining room {} by invitation from {}", room.room_id(), room_member.sender);
        let mut delay = 2;

        while let Err(err) = room.join().await {
            // retry autojoin due to synapse sending invites, before the
            // invited user can join for more information see
            // https://github.com/matrix-org/synapse/issues/4345
            eprintln!("Failed to join room {} ({err:?}), retrying in {delay}s", room.room_id());

            sleep(Duration::from_secs(delay)).await;
            delay *= 2;

            if delay > 3600 {
                eprintln!("Can't join room {} ({err:?})", room.room_id());
                break;
            }
        }
        println!("Successfully joined room {}", room.room_id());
    });
}

async fn handle_message(event: OriginalSyncRoomMessageEvent, room: Room) {
    if room.state() != RoomState::Joined {
        return;
    }
    if event.sender == room.own_user_id() {
        return;
    }
    let MessageType::Text(text_content) = event.content.msgtype else {
        return;
    };

    if text_content.body.starts_with("!ping") {
        println!("Got !ping in {}", room.room_id());
        let content = RoomMessageEventContent::text_plain("I'm here");
        room.send(content).await.unwrap();
    }
}
