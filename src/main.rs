use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use anyhow::Result;
use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct Client 
{
    x: f32,
    y: f32,
    raw_bytes: [u8;44],
    departed_peer_messages: Vec<Message>
}

impl Client 
{
    pub fn new() -> Self
    {
        Self
        {
            x:0.0,
            y:0.0,
            raw_bytes: [0;44],
            departed_peer_messages: Vec::<Message>::new()
        }
    }

    pub fn set_uuid(&mut self, uuid: &String)
    {
        let uuid_bytes = uuid.as_bytes(); //36 bytes

        if uuid_bytes.len() != 36
        {
            println!("A uuid was not 36 bytes long. It was {}",uuid_bytes.len());
            return; //bail!
        }

        let slice_to_overwrite = &mut self.raw_bytes[0..36];
        slice_to_overwrite.copy_from_slice(&uuid.as_bytes());
    }

    pub fn update(&mut self, message: &Message)
    {
        if let Message::Binary(data) = message
        {
            if !message.is_binary() 
            {
                return;
            }

            if message.len() != 8
            {
                return;
            }

            //TODO: x/y is just for debugging
            let bytes1: [u8; 4] = data[0..4].try_into().unwrap();
            self.x = f32::from_le_bytes(bytes1);

            let bytes2: [u8; 4] = data[4..8].try_into().unwrap();
            self.y = f32::from_le_bytes(bytes2);

            (&mut self.raw_bytes[36..40]).copy_from_slice(&bytes1);
            (&mut self.raw_bytes[40..44]).copy_from_slice(&bytes2);
        }
    }

    pub fn print(&self, uuid: &String)
    {
        println!("{} : at {} x {}",uuid,self.x,self.y);
    }

    pub fn as_bytes(&self) -> &[u8;44]
    {
        &self.raw_bytes
    }

    pub fn inform_of_departed_peer(&mut self, uuid: &Uuid)
    {
        let string_uuid = uuid.to_string();
        let uuid_bytes = string_uuid.as_bytes(); //36 bytes. NB: copies, $

        if uuid_bytes.len() != 36
        {
            println!("A uuid was not 36 bytes long. It was {}",uuid_bytes.len());
            return; //bail!
        }

        //A departed peer message is 36 bytes (just a UUID)
        self.departed_peer_messages.push(Message::Binary(uuid_bytes.to_vec()));
    }

    pub fn take_departed_peer_messages(&mut self) -> Vec<Message>
    {
        std::mem::take(&mut self.departed_peer_messages)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:8000".to_string();
    let listener = TcpListener::bind(&addr).await?;

    let peer_db = Arc::new(Mutex::new(HashMap::<Uuid,Client>::new()));

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream,peer_db.clone()));
    }

    Ok(())
}

async fn handle_connection(stream: tokio::net::TcpStream, peer_db: Arc<Mutex<HashMap<Uuid,Client>>>) -> Result<()> {

    let mut ws_stream = accept_async(stream).await?;

    let uuid = Uuid::new_v4();

    {
        let mut db = peer_db.lock().unwrap();

        let mut client = Client::new();
        client.set_uuid(&uuid.to_string()); //NB: copies

        db.insert(uuid,client);
    }

    println!("Client connected (WebSocket): {}", uuid);

    while let Some(msg) = ws_stream.next().await 
    {
        match msg {
            Ok(msg) => {

                if let Message::Close(_) = msg 
                {
                    //Connection is closing...
                    break;
                }
                
                let mut message_queue : Vec<Message> = Vec::<Message>::new(); //TODO: presize?
                //NB: queue is set below, don't append things to it before this occurs.

                {
                    let mut db = peer_db.lock().unwrap();
                    match db.get_mut(&uuid).as_mut()
                    {
                        Some(entry) => 
                        {
                            //Update this client in the server
                            entry.update(&msg);

                            //Drain the departed peer messages into the queue
                            message_queue = entry.take_departed_peer_messages();
                        },
                        None => {}
                    };
                }

                {
                    //Enqueue an update for each connected peer
                    for (peer_uuid,peer) in peer_db.lock().unwrap().iter()
                    {
                        if peer_uuid == &uuid
                        {
                            continue;
                        }

                        message_queue.push(Message::Binary(peer.as_bytes().to_vec()));
                    }
                }

                for message in message_queue.into_iter()
                {
                    ws_stream.send(message).await?;
                }
            },
            Err(_) => {
                //TODO
            }
        };
    }

    println!("Client disconnected (WebSocket): {}", uuid);

    //Remove this peer from the DB
    {
        let mut db = peer_db.lock().unwrap();
        db.remove(&uuid);
    }

    //All the peers in the map need to know this peer left now.
    //When they next poll, they'll get the update 
    {
        for (peer_uuid,peer) in peer_db.lock().unwrap().iter_mut()
        {

            println!("Informed client {} of departing peer {}", peer_uuid, uuid);
            peer.inform_of_departed_peer(&uuid);
        }
    }

    Ok(())
}