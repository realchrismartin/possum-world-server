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
    uuid: Uuid,
    outbound_messages: Vec<Message>
}

impl Client 
{
    pub fn new(uuid: &Uuid) -> Self
    {
        Self
        {
            uuid: uuid.clone(), //Client UUID
            outbound_messages: Vec::<Message>::new()
        }
    }

    pub fn inform_of_departed_peer(&mut self, uuid: &Uuid)
    {
        let binding = uuid.to_string();
        let uuid_bytes = binding.as_bytes(); //36 bytes. NB: copies, $

        if uuid_bytes.len() != 36
        {
            println!("A uuid was not 36 bytes long. It was {}",uuid_bytes.len());
            return; //bail!
        }

        let message_type : i32 = 2;
        let mut message_bytes = Vec::<u8>::new();

        message_bytes.append(&mut message_type.to_le_bytes().to_vec());
        message_bytes.append(&mut uuid_bytes.to_vec());

        //A departed peer message is 4 + 36 bytes
        self.outbound_messages.push(Message::Binary(message_bytes));
    }

    pub fn receive_inbound_message(&mut self, message: &Message)
    {
        //TODO: maybe later dont clone, this wont scale
        self.outbound_messages.push(message.clone());
    }

    pub fn take_outbound_messages(&mut self) -> Vec<Message>
    {
        std::mem::take(&mut self.outbound_messages)
    }
}

fn create_inbound_message(uuid: &Uuid, message: &Message) -> Option<Message>
{
    let binding = uuid.to_string();
    let mut uuid_bytes = binding.as_bytes(); //36 bytes. NB: copies, $

    if uuid_bytes.len() != 36
    {
        println!("A uuid was not 36 bytes long. It was {}",uuid_bytes.len());
        return None; //bail!
    }

    if let Message::Binary(bytes) = message
    {
        if bytes.len() < 4
        {
            //No message type
            return None; //bail
        }

        let mut updated_bytes = Vec::<u8>::new();
        updated_bytes.append(&mut bytes[0..4].to_vec());
        updated_bytes.append(&mut uuid_bytes.to_vec());
        updated_bytes.append(&mut bytes[4..].to_vec());
        return Some(Message::Binary(updated_bytes));
    }
    
    None
}

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "0.0.0.0:8000".to_string();
    let listener = TcpListener::bind(&addr).await?;

    let peer_db = Arc::new(Mutex::new(HashMap::<Uuid,Client>::new()));

    println!("possum-server is now listening on port 8000");

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
        db.insert(uuid,Client::new(&uuid));
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

                //Create an inbound message for this message
                //This tacks the UUID bytes onto the message
                let inbound_message = create_inbound_message(&uuid,&msg);

                {
                    //TODO: maybe later don't do this on EVERY inbound message
                    //Tell all other clients about the message this client sent

                    if inbound_message.is_some()
                    {
                        let inbound_message_unwrapped = inbound_message.unwrap();

                        for (peer_uuid,peer) in peer_db.lock().unwrap().iter_mut()
                        {
                            if peer_uuid == &uuid
                            {
                                continue;
                            }

                            //TODO: this can be very expensive since it's copying the message for every single peer.
                            peer.receive_inbound_message(&inbound_message_unwrapped);
                        }
                    }
                }


                //Send back all messages pending for this client
                let mut outbound_messages = Vec::<Message>::new();

                {
                    let mut db = peer_db.lock().unwrap();
                    match db.get_mut(&uuid).as_mut()
                    {
                        Some(client) => 
                        {
                            outbound_messages = client.take_outbound_messages();
                        },
                        None => {}
                    };
                }

                //Send back all messages which are pending for this client
                for message in outbound_messages
                {
                    ws_stream.send(message).await?;
                }
            },
            Err(_) => {
                //TODO
            }
        };
    }

    //All the peers in the map need to know this peer is leaving now.
    //When they next poll, they'll get the update 
    {
        for (peer_uuid,peer) in peer_db.lock().unwrap().iter_mut()
        {
            if peer_uuid == &uuid
            {
                continue;
            }

            println!("Informed client {} of departing peer {}", peer_uuid, uuid);
            peer.inform_of_departed_peer(&uuid);
        }
    }

    //Remove this peer from the DB
    {
        let mut db = peer_db.lock().unwrap();
        db.remove(&uuid);
    }

    println!("Peer departed (WebSocket closed): {}", uuid);

    Ok(())
}