use config::Config;
use url::Url;
use matrix_sdk::{
    config::SyncSettings,
    event_handler::Ctx,
    matrix_auth::MatrixSession,
    Client, Room, RoomState,
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent,
    },
    ruma::events::room::member::StrippedRoomMemberEvent,
    RoomMemberships,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::time::{sleep, Duration};

mod command;
mod users;
use crate::command::handle_command;
use crate::users::is_user_trusted;

// Things we want to pass to message/event handlers
struct WipContext {
    config: Config,
    allowed_pings: Vec<String>,
    launched_ts: u128,
}

impl Clone for WipContext {
    fn clone(&self) -> Self{
        WipContext {
            config: self.config.clone(),
            allowed_pings: self.allowed_pings.clone(),
            launched_ts: self.launched_ts.clone(),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::builder()
        .add_source(config::File::with_name("config.yaml"))
        .build()
        .unwrap();

    let hs = config.get::<String>("login.homeserver_url").expect("Homeserver url missing in config");
    let hs_url = Url::parse(&hs).expect("Invalid homeserver url");
    let username = config.get::<String>("login.username").expect("Username missing in config");
    let password = config.get::<String>("login.password").expect("Password missing in config");

    let data_dir = dirs::data_dir().expect("no data_dir directory found").join("matrix-wip-bot");
    let db_path = data_dir.join("db");
    let session_path = data_dir.join("session");

    let allowed_pings = config.get::<String>("bot.plaintext_ping").map(|name|
        vec![
            name.clone(),
            format!("{name}:")
        ]
    ).unwrap_or_default();

    let wip_context = WipContext {
        config: config.clone(),
        allowed_pings,
        launched_ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    };

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

        let device_name = config.get::<String>("login.device_name").unwrap_or(String::from("wip-bot"));

        let matrix_auth = client.matrix_auth();
        let login_response = matrix_auth
            .login_username(&username, &password)
            .initial_device_display_name(&device_name)
            .await?;

        println!("Logged in as {}", login_response.device_id);

        let user_session = matrix_auth.session().expect("A logged-in client should have a session");
        let serialized_session = serde_json::to_string(&user_session)?;
        fs::write(session_path, serialized_session).await?;
    }

    client.add_event_handler_context(wip_context);

    // This one is possibly also for old state events handled before
    client.add_event_handler(handle_invites);

    // Sync once without message handler to not deal with old messages
    let sync_response = client.sync_once(SyncSettings::default()).await.unwrap();
    println!("Initial sync finished with token {}, start listening for events", sync_response.next_batch);

    // Actual message handling and sync loop
    client.add_event_handler(handle_message);
    client.sync(SyncSettings::default().token(sync_response.next_batch)).await?;

    Ok(())
}

// From https://github.com/matrix-org/matrix-rust-sdk/blob/main/examples/autojoin/src/main.rs
async fn handle_invites(
    event: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
    wip_context: Ctx<WipContext>
) {
    if event.state_key != client.user_id().unwrap() {
        return;
    }
    if !is_user_trusted(&event.sender, wip_context.0.config) {
        println!("Not auto-joining room {} by untrusted invitation from {}", room.room_id(), event.sender);
        return;
    }

    tokio::spawn(async move {
        println!("Autojoining room {} by invitation from {}", room.room_id(), event.sender);
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

async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    wip_context: Ctx<WipContext>
) {
    if room.state() != RoomState::Joined {
        return;
    }
    if event.sender == room.own_user_id() {
        return;
    }
    let MessageType::Text(text_content) = event.clone().content.msgtype else {
        return;
    };

    if u128::from(event.origin_server_ts.0) < wip_context.0.launched_ts - 10_000 {
        println!("Ignore message in the past: {} in {}", event.event_id, room.room_id());
        return
    }

    let mut split_body = text_content.body.split_whitespace();
    let cmd = split_body.next().unwrap_or_default().to_ascii_lowercase();

    let is_mention = wip_context.0.allowed_pings.iter().any(|ping| ping.to_ascii_lowercase() == cmd);

    let cmd = if is_mention {
        split_body.next().unwrap_or_default().to_ascii_lowercase()
    } else {
        cmd
    };

    // Commands start with '!', or is a mention,
    // or was sent in a DM.
    if cmd.starts_with('!') {
        let cmd = &cmd[1..].to_string();
        handle_command(cmd, split_body, event, room, wip_context.0.config).await;
    } else if is_mention || room.members(RoomMemberships::JOIN).await.unwrap_or_default().len() == 2 {
        handle_command(&cmd, split_body, event, room, wip_context.0.config).await;
    }
}
