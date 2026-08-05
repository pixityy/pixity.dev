// The share's peer connection. Not implemented yet.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rtc::media::Sample;
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::interceptor_registry::register_default_interceptors;
use rtc::peer_connection::configuration::media_engine::{
    MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS,
};
use rtc::peer_connection::configuration::{RTCConfigurationBuilder, RTCIceTransportPolicy};
use rtc::peer_connection::sdp::RTCSessionDescription;
use rtc::peer_connection::transport::RTCIceServer;
use rtc::rtp_transceiver::rtp_sender::{
    RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
    RtpCodecKind,
};
use rtc::rtp_transceiver::PayloadType;
use tokio::sync::Mutex;
use webrtc::media_stream::track_local::static_sample::TrackLocalStaticSample;
use webrtc::media_stream::track_local::TrackLocal;
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCPeerConnectionIceEvent,
};
use webrtc::rtp_transceiver::RtpSender;

use super::{IceServer, Signal};

const STREAM_ID: &str = "pixity-share";

const PT_H264: PayloadType = 102;
const PT_OPUS: PayloadType = 111;

pub type Emit = Arc<dyn Fn(Signal) + Send + Sync>;

#[derive(Clone)]
struct Handler {
    peer: String,
    emit: Emit,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        let Ok(init) = event.candidate.to_json() else { return };
        let Ok(payload) = serde_json::to_value(init) else { return };
        (self.emit)(Signal { peer: self.peer.clone(), kind: "ice".into(), payload });
    }
}

struct Viewer {
    _pc: Arc<dyn PeerConnection>,
    video: Arc<TrackLocalStaticSample>,
    audio: Arc<TrackLocalStaticSample>,
    video_sender: Arc<dyn RtpSender>,
    audio_sender: Arc<dyn RtpSender>,
    video_ssrc: u32,
    audio_ssrc: u32,

    video_pt: Option<PayloadType>,
    audio_pt: Option<PayloadType>,
}

pub struct Peers {
    viewers: Mutex<HashMap<String, Viewer>>,
    ice: Vec<RTCIceServer>,
    emit: Emit,
}

fn codecs() -> (RTCRtpCodecParameters, RTCRtpCodecParameters) {
    let video = RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: MIME_TYPE_H264.to_owned(),
            clock_rate: 90000,
            channels: 0,

            sdp_fmtp_line:
                "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".to_owned(),
            rtcp_feedback: vec![],
        },
        payload_type: PT_H264,
        ..Default::default()
    };
    let audio = RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            clock_rate: 48000,
            channels: 2,

            sdp_fmtp_line: "minptime=10;useinbandfec=1;stereo=1;sprop-stereo=1".to_owned(),
            rtcp_feedback: vec![],
        },
        payload_type: PT_OPUS,
        ..Default::default()
    };
    (video, audio)
}

fn track(
    kind: RtpCodecKind,
    id: &str,
    codec: &RTCRtpCodecParameters,
    ssrc: u32,
) -> Result<Arc<TrackLocalStaticSample>, String> {
    let t = TrackLocalStaticSample::new(MediaStreamTrack::new(
        STREAM_ID.to_owned(),
        id.to_owned(),
        id.to_owned(),
        kind,
        vec![RTCRtpEncodingParameters {
            rtp_coding_parameters: RTCRtpCodingParameters {
                ssrc: Some(ssrc),
                ..Default::default()
            },
            codec: codec.rtp_codec.clone(),
            ..Default::default()
        }],
    ))
    .map_err(|e| e.to_string())?;
    Ok(Arc::new(t))
}

impl Peers {
    pub fn new(turn: &[IceServer], emit: Emit) -> Self {
        let ice = turn
            .iter()
            .map(|s| RTCIceServer {
                urls: s.urls.clone(),
                username: s.username.clone().unwrap_or_default(),
                credential: s.credential.clone().unwrap_or_default(),
                ..Default::default()
            })
            .collect();
        Peers { viewers: Mutex::new(HashMap::new()), ice, emit }
    }

    pub async fn add(&self, peer: &str) -> Result<(), String> {
        let (video_codec, audio_codec) = codecs();

        let mut media = MediaEngine::default();
        media
            .register_codec(video_codec.clone(), RtpCodecKind::Video)
            .map_err(|e| e.to_string())?;
        media
            .register_codec(audio_codec.clone(), RtpCodecKind::Audio)
            .map_err(|e| e.to_string())?;
        let registry = register_default_interceptors(rtc::interceptor::Registry::new(), &mut media)
            .map_err(|e| e.to_string())?;

        let config = RTCConfigurationBuilder::new()
            .with_ice_servers(self.ice.clone())
            .with_ice_transport_policy(RTCIceTransportPolicy::Relay)
            .build();

        let pc = PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_media_engine(media)
            .with_interceptor_registry(registry)
            .with_handler(Arc::new(Handler {
                peer: peer.to_string(),
                emit: Arc::clone(&self.emit),
            }))
            .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let pc: Arc<dyn PeerConnection> = Arc::new(pc);

        let video_ssrc = rand::random::<u32>();
        let audio_ssrc = rand::random::<u32>();
        let video = track(RtpCodecKind::Video, "video", &video_codec, video_ssrc)?;
        let audio = track(RtpCodecKind::Audio, "audio", &audio_codec, audio_ssrc)?;

        let video_sender = pc
            .add_track(Arc::clone(&video) as Arc<dyn TrackLocal>)
            .await
            .map_err(|e| e.to_string())?;
        let audio_sender = pc
            .add_track(Arc::clone(&audio) as Arc<dyn TrackLocal>)
            .await
            .map_err(|e| e.to_string())?;

        let offer = pc.create_offer(None).await.map_err(|e| e.to_string())?;
        pc.set_local_description(offer.clone()).await.map_err(|e| e.to_string())?;
        let payload = serde_json::to_value(&offer).map_err(|e| e.to_string())?;
        (self.emit)(Signal { peer: peer.to_string(), kind: "offer".into(), payload });

        self.viewers.lock().await.insert(
            peer.to_string(),
            Viewer {
                _pc: pc,
                video,
                audio,
                video_sender,
                audio_sender,
                video_ssrc,
                audio_ssrc,
                video_pt: None,
                audio_pt: None,
            },
        );
        Ok(())
    }

    pub async fn signal(&self, sig: Signal) -> Result<(), String> {
        let mut viewers = self.viewers.lock().await;
        let Some(v) = viewers.get_mut(&sig.peer) else {
            return Ok(());
        };
        match sig.kind.as_str() {
            "answer" => {
                let sdp: RTCSessionDescription =
                    serde_json::from_value(sig.payload).map_err(|e| e.to_string())?;
                v._pc.set_remote_description(sdp).await.map_err(|e| e.to_string())?;

                v.video_pt = negotiated(&v.video_sender).await;
                v.audio_pt = negotiated(&v.audio_sender).await;
            }
            "ice" => {
                let init = serde_json::from_value(sig.payload).map_err(|e| e.to_string())?;
                v._pc.add_ice_candidate(init).await.map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn remove(&self, peer: &str) {
        if let Some(v) = self.viewers.lock().await.remove(peer) {
            let _ = v._pc.close().await;
        }
    }

    pub async fn close(&self) {
        let mut viewers = self.viewers.lock().await;
        for (_, v) in viewers.drain() {
            let _ = v._pc.close().await;
        }
    }

    pub async fn write_video(&self, data: Vec<u8>, frame: Duration) {
        let payload = bytes::Bytes::from(data);
        for v in self.viewers.lock().await.values() {
            let Some(pt) = v.video_pt else { continue };
            let _ = v
                .video
                .sample_writer(v.video_ssrc, pt)
                .write_sample(&Sample {
                    data: payload.clone(),
                    duration: frame,
                    ..Default::default()
                })
                .await;
        }
    }

    pub async fn write_audio(&self, data: Vec<u8>) {
        let payload = bytes::Bytes::from(data);
        for v in self.viewers.lock().await.values() {
            let Some(pt) = v.audio_pt else { continue };
            let _ = v
                .audio
                .sample_writer(v.audio_ssrc, pt)
                .write_sample(&Sample {
                    data: payload.clone(),

                    duration: Duration::from_millis(20),
                    ..Default::default()
                })
                .await;
        }
    }
}

async fn negotiated(sender: &Arc<dyn RtpSender>) -> Option<PayloadType> {
    sender
        .get_parameters()
        .await
        .ok()?
        .rtp_parameters
        .codecs
        .first()
        .map(|c| c.payload_type)
}
