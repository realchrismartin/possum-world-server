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
    y: f32
}

impl Client 
{
    pub fn new() -> Self
    {
        Self
        {
            x:0.0,
            y:0.0
        }
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

            let bytes1: [u8; 4] = data[0..4].try_into().unwrap();
            self.x = f32::from_le_bytes(bytes1);

            let bytes2: [u8; 4] = data[4..8].try_into().unwrap();
            self.y = f32::from_le_bytes(bytes2);
        }
    }

    pub fn print(&self, uuid: &String)
    {
        println!("{} : at {} x {}",uuid,self.x,self.y);
    }

    /*
    pub fn from_vec_u8(&self) -> Vec<u8>
    {
        vec![1,2,3]
    }
    */
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "127.0.0.1:8000".to_string();
    let listener = TcpListener::bind(&addr).await?;

    let peer_db = Arc::new(Mutex::new(HashMap::<String,Client>::new()));

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream,peer_db.clone()));
    }

    Ok(())
}

async fn handle_connection(stream: tokio::net::TcpStream, peer_db: Arc<Mutex<HashMap<String,Client>>>) -> Result<()> {

    let mut ws_stream = accept_async(stream).await?;

    let uuid = Uuid::new_v4();

    {
        let mut db = peer_db.lock().unwrap();
        db.insert(uuid.to_string().clone(),Client::new());
    }

    println!("Client connected (WebSocket): {}", uuid);

    while let Some(msg) = ws_stream.next().await 
    {
        match msg {
            Ok(msg) => {
                
                {
                    let mut db = peer_db.lock().unwrap();
                    match db.get_mut(&uuid.to_string()).as_mut()
                    {
                        Some(entry) => 
                        {
                            entry.update(&msg);
                            entry.print(&uuid.to_string());
                        },
                        None => {}
                    };
                }

                /*
                let maybe_data :  Option<Vec<u8>>;

                match maybe_data
                {
                    Some(d) => {
                        //println!("Sending back data we saved.");
                        ws_stream.send(Message::Binary(d)).await?;
                    },
                    None => {}
                }
                */
            },
            Err(e) => {
                //TODO
            }
        };
    }

    {
        let mut db = peer_db.lock().unwrap();
        db.remove(&uuid.to_string());
    }

    println!("Client disconnected (WebSocket): {}", uuid);

    Ok(())
}