use anyhow::{anyhow, Result};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use local_ip_address::local_ip;
use std::collections::HashMap;
use std::net::IpAddr;

pub struct DiscoveryService {
    daemon: ServiceDaemon,
    service_info: Option<ServiceInfo>,
}

impl DiscoveryService {
    pub fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new()?;
        Ok(Self {
            daemon,
            service_info: None,
        })
    }

    pub fn register_service(
        &mut self,
        service_name: &str,
        port: u16,
        server_url: Option<&str>,
    ) -> Result<()> {
        // Get local IP address
        let local_ip = local_ip().map_err(|e| anyhow!("Failed to get local IP: {}", e))?;

        // Prepare TXT record with metadata
        let mut properties = HashMap::new();
        properties.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());
        if let Some(url) = server_url {
            properties.insert("server".to_string(), url.to_string());
        }

        // Create service info
        // The hostname parameter needs to end with '.local.'
        let hostname = format!("{}.local.", service_name);
        let service_info = ServiceInfo::new(
            "_pinepods-remote._tcp.local.",
            service_name,  // Instance name (just the name)
            &hostname,     // Hostname (must end with .local.)
            local_ip,
            port,
            properties,
        )?;

        // Register the service
        self.daemon.register(service_info.clone())?;
        self.service_info = Some(service_info);
        
        log::info!("Registered mDNS service: {} on {}:{}", service_name, local_ip, port);
        Ok(())
    }

    pub fn unregister_service(&mut self) -> Result<()> {
        if let Some(service_info) = &self.service_info {
            // Try to unregister but ignore errors since we're likely shutting down
            if let Err(e) = self.daemon.unregister(service_info.get_fullname()) {
                // Only log debug level during shutdown to reduce noise
                log::debug!("mDNS unregistration failed (expected during shutdown): {}", e);
            } else {
                log::info!("Unregistered mDNS service");
            }
            self.service_info = None;
        }
        Ok(())
    }
}

impl Drop for DiscoveryService {
    fn drop(&mut self) {
        // Try graceful unregistration but don't log errors during shutdown
        let _ = self.unregister_service();
    }
}

pub async fn discover_remote_players(timeout_secs: u64) -> Result<Vec<RemotePlayerDiscovered>> {
    let daemon = ServiceDaemon::new()?;
    let receiver = daemon.browse("_pinepods-remote._tcp.local.")?;
    
    let mut players = Vec::new();
    let start_time = std::time::Instant::now();
    
    while start_time.elapsed().as_secs() < timeout_secs {
        if let Ok(event) = receiver.recv_timeout(std::time::Duration::from_secs(1)) {
            match event {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    let properties: HashMap<String, String> = info.get_properties()
                        .iter()
                        .map(|prop| (prop.key().to_string(), prop.val_str().to_string()))
                        .collect();
                    
                    let player = RemotePlayerDiscovered {
                        name: info.get_fullname().to_string(),
                        host: format!("{}", info.get_addresses().iter().next().unwrap_or(&IpAddr::from([127, 0, 0, 1]))),
                        port: info.get_port(),
                        properties,
                    };
                    players.push(player.clone());
                    log::info!("Discovered remote player: {} at {}:{}", player.name, player.host, player.port);
                }
                _ => {}
            }
        }
    }
    
    Ok(players)
}

#[derive(Debug, Clone)]
pub struct RemotePlayerDiscovered {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub properties: HashMap<String, String>,
}

impl RemotePlayerDiscovered {
    pub fn get_base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
    
    pub fn get_property(&self, key: &str) -> Option<&String> {
        self.properties.get(key)
    }
}