use std::cell::RefCell;
use futures::StreamExt;
use js_sys::Uint8Array;
use ractor::thread_local::ThreadLocalActorSpawner;
use ractor::{ActorRef, RpcReplyPort};
use ractor::rpc::CallResult;
use ractor_wormhole::util::ThreadLocalFnActor;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    BluetoothDevice, BluetoothRemoteGattCharacteristic,
    BluetoothRemoteGattServer, BluetoothRemoteGattService, window,
};

// Global state for the Bluetooth connection
thread_local! {
    static BLUETOOTH_ACTOR: RefCell<Option<ActorRef<BluetoothMsg>>> = RefCell::new(None);
}

// Characteristic info for UI display
#[derive(Clone, Debug)]
pub struct CharacteristicInfo {
    pub uuid: String,
    pub properties: Vec<String>,
}

// Message types for the Bluetooth actor
#[derive(Debug)]
pub enum BluetoothMsg {
    GetCharacteristics(RpcReplyPort<Result<Vec<CharacteristicInfo>, String>>),
    ReadCharacteristic(String, RpcReplyPort<Result<String, String>>),
    WriteCharacteristic(String, Vec<u8>, RpcReplyPort<Result<(), String>>),
    Disconnect,
}

// NOTE: BluetoothMsg does NOT implement Message because BluetoothDevice is !Send
// We use it directly with futures::channel::mpsc which doesn't require Send

pub fn render_bluetooth_page() {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let body = document.body().expect("no body");

    body.set_inner_html(r#"
        <div class="container">
            <h1>🔵 WebBluetooth Demo</h1>
            
            <div class="test-section">
                <h2>Navigation</h2>
                <button id="backButton">← Back to Tests</button>
            </div>

            <div class="test-section">
                <h2>Bluetooth Connection</h2>
                <p>This demo shows how to use <strong>non-Send</strong> WebBluetooth objects with ThreadLocalFnActor.</p>
                <p>Click Connect to scan for nearby Bluetooth devices.</p>
                <button id="connectButton" class="bt-button">🔍 Connect to Bluetooth Device</button>
                <button id="disconnectButton" class="bt-button" disabled>🔌 Disconnect</button>
                <div id="connectionStatus" class="status">Not connected</div>
            </div>

            <div class="test-section">
                <h2>Device Information</h2>
                <div id="deviceInfo" class="log">
                    <div class="log-entry info">No device connected</div>
                </div>
            </div>

            <div class="test-section">
                <h2>GATT Characteristics</h2>
                <div id="characteristicsList" class="log">
                    <div class="log-entry info">Connect to see characteristics</div>
                </div>
            </div>
        </div>
    "#);

    setup_bluetooth_handlers();
}

fn setup_bluetooth_handlers() {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");

    // Back button
    let back_button = document
        .get_element_by_id("backButton")
        .expect("back button not found");
    let closure = Closure::wrap(Box::new(move || {
        crate::render_main_page();
    }) as Box<dyn FnMut()>);
    back_button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();

    // Connect button
    let connect_button = document
        .get_element_by_id("connectButton")
        .expect("connect button not found");
    let closure = Closure::wrap(Box::new(move || {
        wasm_bindgen_futures::spawn_local(async {
            if let Err(e) = connect_to_bluetooth_device().await {
                log_to_device_info(&format!("❌ Connection failed: {}", e), "error");
                set_bluetooth_status("Connection failed", "error");
            }
        });
    }) as Box<dyn FnMut()>);
    connect_button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();

    // Disconnect button
    let disconnect_button = document
        .get_element_by_id("disconnectButton")
        .expect("disconnect button not found");
    let closure = Closure::wrap(Box::new(move || {
        // Send disconnect message to actor
        BLUETOOTH_ACTOR.with(|actor| {
            if let Some(actor_ref) = actor.borrow().as_ref() {
                let _ = actor_ref.send_message(BluetoothMsg::Disconnect);
            }
        });
        set_bluetooth_status("Disconnected", "");
        update_ui_connection_state(false);
        clear_characteristics_list();
    }) as Box<dyn FnMut()>);
    disconnect_button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();
}

async fn connect_to_bluetooth_device() -> Result<(), String> {
    log_to_device_info("🔍 Requesting Bluetooth device...", "info");
    set_bluetooth_status("Scanning...", "running");

    let window = window().ok_or("No window")?;
    let navigator = window.navigator();
    let bluetooth = navigator
        .bluetooth()
        .ok_or("Bluetooth not available - try Chrome/Edge")?;

    // Create request options - accept all devices for demo
    let options = web_sys::RequestDeviceOptions::new();
    options.set_accept_all_devices(true);
    
    // Request optional services (generic GATT services)
    let optional_services = js_sys::Array::new();
    optional_services.push(&JsValue::from_str("battery_service"));
    optional_services.push(&JsValue::from_str("device_information"));
    optional_services.push(&JsValue::from_str("generic_access"));
    optional_services.push(&JsValue::from_str("heart_rate"));
    options.set_optional_services(&optional_services);

    // Request device
    let device_promise = bluetooth.request_device(&options);
    let device: BluetoothDevice = wasm_bindgen_futures::JsFuture::from(device_promise)
        .await
        .map_err(|e| format!("Failed to request device: {:?}", e))?
        .unchecked_into();

    let device_name = device.name().unwrap_or_else(|| "Unknown Device".to_string());
    let device_id = device.id();

    log_to_device_info(&format!("✅ Selected device: {}", device_name), "success");
    log_to_device_info(&format!("📱 Device ID: {}", device_id), "info");
    set_bluetooth_status(&format!("Connecting to {}", device_name), "running");
    update_ui_connection_state(false); // Still connecting

    // Connect to GATT server
    log_to_device_info("🔗 Connecting to GATT server...", "info");
    let gatt = device.gatt().ok_or("No GATT server")?;
    let server_promise = gatt.connect();
    let server: BluetoothRemoteGattServer = wasm_bindgen_futures::JsFuture::from(server_promise)
        .await
        .map_err(|e| format!("Failed to connect GATT: {:?}", e))?
        .unchecked_into();

    log_to_device_info("✅ GATT server connected", "success");
    set_bluetooth_status(&format!("Connected to {}", device_name), "success");
    update_ui_connection_state(true);

    // Start the actor to handle device interactions
    let actor_ref = spawn_bluetooth_actor(device.clone(), server.clone()).await?;
    
    // Store the actor reference globally
    BLUETOOTH_ACTOR.with(|actor| {
        *actor.borrow_mut() = Some(actor_ref.clone());
    });

    // Discover characteristics via the actor
    log_to_device_info("🔍 Requesting characteristics from actor...", "info");
    match actor_ref.call(|reply| BluetoothMsg::GetCharacteristics(reply), None).await {
        Ok(CallResult::Success(Ok(characteristics))) => {
            log_to_device_info(&format!("📡 Found {} characteristic(s)", characteristics.len()), "success");
            display_characteristics(characteristics);
        }
        Ok(CallResult::Success(Err(e))) => {
            log_to_device_info(&format!("❌ Failed to get characteristics: {}", e), "error");
        }
        Ok(CallResult::Timeout) => {
            log_to_device_info("❌ Actor call timed out", "error");
        }
        Ok(CallResult::SenderError) => {
            log_to_device_info("❌ Actor sender error", "error");
        }
        Err(e) => {
            log_to_device_info(&format!("❌ Actor call failed: {:?}", e), "error");
        }
    }

    Ok(())
}

async fn spawn_bluetooth_actor(
    device: BluetoothDevice,
    server: BluetoothRemoteGattServer,
) -> Result<ActorRef<BluetoothMsg>, String> {
    log_to_device_info("🎬 Starting Bluetooth actor...", "info");

    let spawner = ThreadLocalActorSpawner::new();
    
    let (actor_ref, _handle) = ThreadLocalFnActor::<BluetoothMsg>::start_fn(
        spawner,
        |mut ctx| async move {
            log_to_device_info("✅ Bluetooth actor started!", "success");
            log_to_device_info(
                &format!("💡 Actor managing device: {} (BluetoothDevice is !Send!)", 
                    device.name().unwrap_or_default()), 
                "info"
            );
            log_to_device_info(
                "💡 This works because everything stays on the main thread!", 
                "success"
            );
            
            // Store discovered characteristics in the actor's context
            let mut characteristics: Vec<(String, BluetoothRemoteGattCharacteristic)> = Vec::new();
            
            while let Some(msg) = ctx.rx.next().await {
                match msg {
                    BluetoothMsg::GetCharacteristics(reply) => {
                        log_to_device_info("� Actor discovering services...", "info");
                        
                        match discover_characteristics_internal(&server).await {
                            Ok(discovered) => {
                                // Store characteristics for later use
                                characteristics = discovered.clone();
                                
                                // Convert to CharacteristicInfo for UI
                                let info_list: Vec<CharacteristicInfo> = discovered
                                    .iter()
                                    .map(|(uuid, ch)| {
                                        let props = ch.properties();
                                        let mut prop_list = Vec::new();
                                        if props.read() { prop_list.push("Read".to_string()); }
                                        if props.write() { prop_list.push("Write".to_string()); }
                                        if props.write_without_response() { prop_list.push("WriteNoResp".to_string()); }
                                        if props.notify() { prop_list.push("Notify".to_string()); }
                                        if props.indicate() { prop_list.push("Indicate".to_string()); }
                                        
                                        CharacteristicInfo {
                                            uuid: uuid.clone(),
                                            properties: prop_list,
                                        }
                                    })
                                    .collect();
                                
                                log_to_device_info(
                                    &format!("✅ Actor discovered {} characteristics", info_list.len()),
                                    "success"
                                );
                                let _ = reply.send(Ok(info_list));
                            }
                            Err(e) => {
                                log_to_device_info(&format!("❌ Discovery failed: {}", e), "error");
                                let _ = reply.send(Err(e));
                            }
                        }
                    }
                    BluetoothMsg::ReadCharacteristic(uuid, reply) => {
                        // Find the characteristic
                        if let Some((_, ch)) = characteristics.iter().find(|(u, _)| u == &uuid) {
                            let ch_clone = ch.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                let result = try_read_characteristic(&ch_clone).await;
                                let _ = reply.send(result);
                            });
                        } else {
                            let _ = reply.send(Err(format!("Characteristic {} not found", uuid)));
                        }
                    }
                    BluetoothMsg::WriteCharacteristic(uuid, data, reply) => {
                        // Find the characteristic
                        if let Some((_, ch)) = characteristics.iter().find(|(u, _)| u == &uuid) {
                            let ch_clone = ch.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                let result = try_write_characteristic(&ch_clone, &data).await;
                                let _ = reply.send(result);
                            });
                        } else {
                            let _ = reply.send(Err(format!("Characteristic {} not found", uuid)));
                        }
                    }
                    BluetoothMsg::Disconnect => {
                        log_to_device_info("🔌 Disconnecting...", "info");
                        if let Some(gatt) = device.gatt() {
                            if gatt.connected() {
                                gatt.disconnect();
                            }
                        }
                        BLUETOOTH_ACTOR.with(|actor| {
                            *actor.borrow_mut() = None;
                        });
                        break;
                    }
                }
            }
            
            log_to_device_info("Bluetooth actor stopped", "info");
        },
    )
    .await
    .map_err(|e| format!("Failed to start actor: {:?}", e))?;

    log_to_device_info("🎉 Actor is managing the Bluetooth device!", "success");

    Ok(actor_ref)
}

async fn discover_characteristics_internal(server: &BluetoothRemoteGattServer) -> Result<Vec<(String, BluetoothRemoteGattCharacteristic)>, String> {
    log_to_device_info("🔍 Discovering services...", "info");

    let services_promise = server.get_primary_services();
    let services_js = wasm_bindgen_futures::JsFuture::from(services_promise)
        .await
        .map_err(|e| format!("Failed to get services: {:?}", e))?;

    let services = js_sys::Array::from(&services_js);
    log_to_device_info(&format!("📡 Found {} service(s)", services.length()), "success");

    let mut result = Vec::new();

    for i in 0..services.length() {
        let service: BluetoothRemoteGattService = services
            .get(i)
            .unchecked_into();
        let service_uuid = service.uuid();

        log_to_characteristics(&format!("📦 Service: {}", service_uuid), "info");

        // Get characteristics for this service
        let chars_promise = service.get_characteristics();
        let chars_result = wasm_bindgen_futures::JsFuture::from(chars_promise).await;
        
        if let Ok(chars_js) = chars_result {
            let chars = js_sys::Array::from(&chars_js);

            for j in 0..chars.length() {
                let ch: BluetoothRemoteGattCharacteristic = chars
                    .get(j)
                    .unchecked_into();
                let char_uuid = ch.uuid();
                let props = ch.properties();

                let mut prop_list = Vec::new();
                if props.read() {
                    prop_list.push("Read".to_string());
                }
                if props.write() {
                    prop_list.push("Write".to_string());
                }
                if props.write_without_response() {
                    prop_list.push("WriteNoResp".to_string());
                }
                if props.notify() {
                    prop_list.push("Notify".to_string());
                }
                if props.indicate() {
                    prop_list.push("Indicate".to_string());
                }

                let props_str = prop_list.join(", ");
                log_to_characteristics(
                    &format!("  ⚡ {}: [{}]", char_uuid, props_str),
                    "success",
                );

                // Store for return
                result.push((char_uuid.clone(), ch.clone()));
                
                // Try to enable notifications if supported
                if props.notify() {
                    let ch_clone = ch.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let promise = ch_clone.start_notifications();
                        if let Ok(_) = wasm_bindgen_futures::JsFuture::from(promise).await {
                                log_to_characteristics(
                                    &format!("    � Notifications enabled for {}", ch_clone.uuid()),
                                    "success"
                                );
                                
                                // Set up notification handler
                                let closure = Closure::wrap(Box::new(move |ev: JsValue| {
                                    if let Some(hex) = extract_value_from_event(&ev) {
                                        log_to_device_info(
                                            &format!("📬 Notification: {} bytes", hex.split(' ').count()),
                                            "success"
                                        );
                                    }
                                }) as Box<dyn FnMut(_)>);
                                
                                let _ = ch_clone.add_event_listener_with_callback(
                                    "characteristicvaluechanged",
                                    closure.as_ref().unchecked_ref()
                                );
                                closure.forget();
                        }
                    });
                }
            }
        }
    }

    Ok(result)
}


// UI function to display characteristics
fn display_characteristics(characteristics: Vec<CharacteristicInfo>) {
    clear_characteristics_list();
    
    for char_info in characteristics {
        let props_str = char_info.properties.join(", ");
        log_to_characteristics(
            &format!("⚡ {}: [{}]", char_info.uuid, props_str),
            "success",
        );

        // Add read button if readable
        if char_info.properties.contains(&"Read".to_string()) {
            add_read_button(&char_info.uuid);
        }
        
        // Add write button if writable
        if char_info.properties.contains(&"Write".to_string()) || 
           char_info.properties.contains(&"WriteNoResp".to_string()) {
            add_write_button(&char_info.uuid);
        }
    }
}

// Try to read a characteristic value
async fn try_read_characteristic(ch: &BluetoothRemoteGattCharacteristic) -> Result<String, String> {
    let value_promise = ch.read_value();
    let value_js = wasm_bindgen_futures::JsFuture::from(value_promise)
        .await
        .map_err(|e| format!("Read failed: {:?}", e))?;
    
    // The returned value is a DataView, we need to extract the buffer
    // Try to get the buffer and create a Uint8Array from it
    if let Ok(buffer) = js_sys::Reflect::get(&value_js, &JsValue::from_str("buffer")) {
        let byte_offset = js_sys::Reflect::get(&value_js, &JsValue::from_str("byteOffset"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32;
        let byte_length = js_sys::Reflect::get(&value_js, &JsValue::from_str("byteLength"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32;
        
        if byte_length == 0 {
            return Ok("(empty)".to_string());
        }
        
        let arr = Uint8Array::new(&buffer);
        let view = arr.subarray(byte_offset, byte_offset + byte_length);
        return Ok(bytes_to_hex(&view));
    }
    
    // Fallback: try as direct Uint8Array
    let u8array = Uint8Array::new(&value_js);
    if u8array.length() == 0 {
        Ok("(empty)".to_string())
    } else {
        Ok(bytes_to_hex(&u8array))
    }
}

// Try to write to a characteristic
async fn try_write_characteristic(ch: &BluetoothRemoteGattCharacteristic, data: &[u8]) -> Result<(), String> {
    let u8array = Uint8Array::from(data);
    let write_promise = ch.write_value_with_u8_array(&u8array)
        .map_err(|e| format!("Write setup failed: {:?}", e))?;
    
    wasm_bindgen_futures::JsFuture::from(write_promise)
        .await
        .map_err(|e| format!("Write failed: {:?}", e))?;
    
    Ok(())
}

// Convert Uint8Array to hex string
fn bytes_to_hex(u8a: &Uint8Array) -> String {
    let mut hex = String::new();
    for i in 0..u8a.length() {
        if !hex.is_empty() {
            hex.push(' ');
        }
        hex.push_str(&format!("{:02x}", u8a.get_index(i)));
    }
    hex
}

// Extract value from a characteristic value changed event
fn extract_value_from_event(ev: &JsValue) -> Option<String> {
    // Try target.value
    let mut value = js_sys::Reflect::get(ev, &JsValue::from_str("target"))
        .ok()
        .and_then(|t| js_sys::Reflect::get(&t, &JsValue::from_str("value")).ok())
        .filter(|v| !v.is_null() && !v.is_undefined());

    // Try ev.value
    if value.is_none() {
        value = js_sys::Reflect::get(ev, &JsValue::from_str("value"))
            .ok()
            .filter(|v| !v.is_null() && !v.is_undefined());
    }

    let value = value?;

    // Handle DataView
    if let Ok(buffer) = js_sys::Reflect::get(&value, &JsValue::from_str("buffer")) {
        if !buffer.is_null() {
            let byte_offset = js_sys::Reflect::get(&value, &JsValue::from_str("byteOffset"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32;
            let byte_length = js_sys::Reflect::get(&value, &JsValue::from_str("byteLength"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32;
            let arr = Uint8Array::new(&buffer);
            let view = arr.subarray(byte_offset, byte_offset + byte_length);
            return Some(bytes_to_hex(&view));
        }
    }

    // Fallback: try direct Uint8Array
    Some(bytes_to_hex(&Uint8Array::new(&value)))
}

fn log_to_device_info(message: &str, class: &str) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let log_div = document
        .get_element_by_id("deviceInfo")
        .expect("deviceInfo div not found");

    let entry = document.create_element("div").expect("create div");
    entry.set_class_name(&format!("log-entry {}", class));
    entry.set_inner_html(message);

    log_div.append_child(&entry).expect("append child");
    log_div.set_scroll_top(log_div.scroll_height());
}

fn log_to_characteristics(message: &str, class: &str) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let log_div = document
        .get_element_by_id("characteristicsList")
        .expect("characteristicsList div not found");

    let entry = document.create_element("div").expect("create div");
    entry.set_class_name(&format!("log-entry {}", class));
    entry.set_inner_html(message);

    log_div.append_child(&entry).expect("append child");
    log_div.set_scroll_top(log_div.scroll_height());
}

fn clear_characteristics_list() {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let log_div = document
        .get_element_by_id("characteristicsList")
        .expect("characteristicsList div not found");
    log_div.set_inner_html("<div class='log-entry info'>Connect to see characteristics</div>");
}

fn add_read_button(char_uuid: &str) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let log_div = document
        .get_element_by_id("characteristicsList")
        .expect("characteristicsList div not found");

    let button = document.create_element("button").expect("create button");
    button.set_class_name("bt-read-button");
    button.set_inner_html(&format!("📖 Read {}", char_uuid));
    
    let uuid = char_uuid.to_string();
    let closure = Closure::wrap(Box::new(move || {
        let uuid_clone = uuid.clone();
        // Call actor via RPC
        BLUETOOTH_ACTOR.with(|actor| {
            if let Some(actor_ref) = actor.borrow().as_ref() {
                let actor_clone = actor_ref.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    log_to_device_info(&format!("📖 Reading {}...", uuid_clone), "info");
                    match actor_clone.call(
                        |reply| BluetoothMsg::ReadCharacteristic(uuid_clone.clone(), reply),
                        None
                    ).await {
                        Ok(CallResult::Success(Ok(hex))) => {
                            log_to_device_info(&format!("✅ Read value: {}", hex), "success");
                        }
                        Ok(CallResult::Success(Err(e))) => {
                            log_to_device_info(&format!("❌ Read error: {}", e), "error");
                        }
                        Ok(CallResult::Timeout) => {
                            log_to_device_info("❌ Read timed out", "error");
                        }
                        Ok(CallResult::SenderError) => {
                            log_to_device_info("❌ Sender error", "error");
                        }
                        Err(e) => {
                            log_to_device_info(&format!("❌ Actor call failed: {:?}", e), "error");
                        }
                    }
                });
            } else {
                log_to_device_info("❌ Bluetooth actor not available", "error");
            }
        });
    }) as Box<dyn FnMut()>);
    
    button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();

    log_div.append_child(&button).expect("append child");
}

fn add_write_button(char_uuid: &str) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let log_div = document
        .get_element_by_id("characteristicsList")
        .expect("characteristicsList div not found");

    let button = document.create_element("button").expect("create button");
    button.set_class_name("bt-read-button");
    button.set_inner_html(&format!("✍️ Write Test to {}", char_uuid));
    
    let uuid = char_uuid.to_string();
    let closure = Closure::wrap(Box::new(move || {
        let uuid_clone = uuid.clone();
        // Call actor via RPC with test data
        let test_data = vec![0x01, 0x02, 0x03, 0x04];
        BLUETOOTH_ACTOR.with(|actor| {
            if let Some(actor_ref) = actor.borrow().as_ref() {
                let actor_clone = actor_ref.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    log_to_device_info(
                        &format!("✍️ Writing {} bytes to {}...", test_data.len(), uuid_clone),
                        "info"
                    );
                    match actor_clone.call(
                        |reply| BluetoothMsg::WriteCharacteristic(uuid_clone.clone(), test_data, reply),
                        None
                    ).await {
                        Ok(CallResult::Success(Ok(()))) => {
                            log_to_device_info("✅ Write successful", "success");
                        }
                        Ok(CallResult::Success(Err(e))) => {
                            log_to_device_info(&format!("❌ Write error: {}", e), "error");
                        }
                        Ok(CallResult::Timeout) => {
                            log_to_device_info("❌ Write timed out", "error");
                        }
                        Ok(CallResult::SenderError) => {
                            log_to_device_info("❌ Sender error", "error");
                        }
                        Err(e) => {
                            log_to_device_info(&format!("❌ Actor call failed: {:?}", e), "error");
                        }
                    }
                });
            } else {
                log_to_device_info("❌ Bluetooth actor not available", "error");
            }
        });
    }) as Box<dyn FnMut()>);
    
    button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();

    log_div.append_child(&button).expect("append child");
}

fn set_bluetooth_status(message: &str, class: &str) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let status_div = document
        .get_element_by_id("connectionStatus")
        .expect("connectionStatus div not found");

    status_div.set_inner_html(message);
    status_div.set_class_name(&format!("status {}", class));
}

fn update_ui_connection_state(connected: bool) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    
    if let Some(connect_btn) = document.get_element_by_id("connectButton") {
        let button: web_sys::HtmlButtonElement = connect_btn.dyn_into().unwrap();
        button.set_disabled(connected);
    }
    
    if let Some(disconnect_btn) = document.get_element_by_id("disconnectButton") {
        let button: web_sys::HtmlButtonElement = disconnect_btn.dyn_into().unwrap();
        button.set_disabled(!connected);
    }
}
