use futures::StreamExt;
use ractor::thread_local::ThreadLocalActorSpawner;
use ractor_wormhole::util::ThreadLocalFnActor;
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;
use web_sys::{console, window};

mod bluetooth_page;

// Setup panic hook for better error messages
fn setup_panic_hook() {
    console_error_panic_hook::set_once();
}

// Helper function to log to the web page
fn log_to_page(message: &str, class: &str) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let log_div = document
        .get_element_by_id("log")
        .expect("log div not found");

    let entry = document.create_element("div").expect("create div");
    entry.set_class_name(&format!("log-entry {}", class));
    entry.set_inner_html(message);

    log_div.append_child(&entry).expect("append child");

    // Auto-scroll to bottom
    log_div.set_scroll_top(log_div.scroll_height());
}

fn set_status(message: &str, class: &str) {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");
    let status_div = document
        .get_element_by_id("status")
        .expect("status div not found");

    status_div.set_inner_html(message);
    status_div.set_class_name(&format!("status {}", class));
}

// Test 1: Basic message passing
async fn test_basic_message_passing() -> Result<(), String> {
    log_to_page("🧪 Test 1: Basic message passing", "info");

    let spawner = ThreadLocalActorSpawner::new();
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let (actor_ref, _handle) = ThreadLocalFnActor::<u32>::start_fn(spawner, |mut ctx| async move {
        while let Some(msg) = ctx.rx.next().await {
            received_clone.lock().unwrap().push(msg);
        }
    })
    .await
    .map_err(|e| format!("Failed to start actor: {:?}", e))?;

    // Send some messages
    actor_ref
        .send_message(42)
        .map_err(|e| format!("Failed to send: {:?}", e))?;
    actor_ref
        .send_message(100)
        .map_err(|e| format!("Failed to send: {:?}", e))?;
    actor_ref
        .send_message(999)
        .map_err(|e| format!("Failed to send: {:?}", e))?;

    // Give it a moment to process
    wait_ms(100).await;

    let messages = received.lock().unwrap();
    if messages.len() != 3 {
        return Err(format!("Expected 3 messages, got {}", messages.len()));
    }
    if messages[0] != 42 || messages[1] != 100 || messages[2] != 999 {
        return Err(format!("Messages mismatch: {:?}", *messages));
    }

    log_to_page("✅ Basic message passing works!", "success");
    Ok(())
}

// Test 2: Multiple actors
async fn test_multiple_actors() -> Result<(), String> {
    log_to_page("🧪 Test 2: Multiple actors", "info");

    let spawner = ThreadLocalActorSpawner::new();

    let counter1 = Arc::new(Mutex::new(0));
    let counter2 = Arc::new(Mutex::new(0));

    let c1 = counter1.clone();
    let (actor1, _handle1) =
        ThreadLocalFnActor::<u32>::start_fn(spawner.clone(), |mut ctx| async move {
            while let Some(msg) = ctx.rx.next().await {
                *c1.lock().unwrap() += msg;
            }
        })
        .await
        .map_err(|e| format!("Failed to start actor1: {:?}", e))?;

    let c2 = counter2.clone();
    let (actor2, _handle2) = ThreadLocalFnActor::<u32>::start_fn(spawner, |mut ctx| async move {
        while let Some(msg) = ctx.rx.next().await {
            *c2.lock().unwrap() += msg;
        }
    })
    .await
    .map_err(|e| format!("Failed to start actor2: {:?}", e))?;

    // Send to both actors
    actor1.send_message(10).unwrap();
    actor1.send_message(20).unwrap();
    actor2.send_message(5).unwrap();
    actor2.send_message(15).unwrap();

    wait_ms(100).await;

    let c1_val = *counter1.lock().unwrap();
    let c2_val = *counter2.lock().unwrap();

    if c1_val != 30 {
        return Err(format!("Actor1 counter expected 30, got {}", c1_val));
    }
    if c2_val != 20 {
        return Err(format!("Actor2 counter expected 20, got {}", c2_val));
    }

    log_to_page("✅ Multiple actors work independently!", "success");
    Ok(())
}

// Test 3: String messages (WASM-friendly)
async fn test_string_messages() -> Result<(), String> {
    log_to_page("🧪 Test 3: String messages", "info");

    let spawner = ThreadLocalActorSpawner::new();
    let results = Arc::new(Mutex::new(Vec::new()));
    let results_clone = results.clone();

    let (actor_ref, _handle) =
        ThreadLocalFnActor::<String>::start_fn(spawner, |mut ctx| async move {
            while let Some(msg) = ctx.rx.next().await {
                results_clone.lock().unwrap().push(msg);
            }
        })
        .await
        .map_err(|e| format!("Failed to start actor: {:?}", e))?;

    // Send string messages
    actor_ref.send_message("Hello".to_string()).unwrap();
    actor_ref.send_message("WASM".to_string()).unwrap();
    actor_ref.send_message("World".to_string()).unwrap();

    wait_ms(100).await;

    let res = results.lock().unwrap();
    if res.len() != 3 {
        return Err(format!("Expected 3 results, got {}", res.len()));
    }
    if res[0] != "Hello" || res[1] != "WASM" || res[2] != "World" {
        return Err(format!("Results mismatch: {:?}", *res));
    }

    log_to_page("✅ String messages work perfectly!", "success");
    Ok(())
}

// Test 4: High message throughput
async fn test_high_throughput() -> Result<(), String> {
    log_to_page("🧪 Test 4: High message throughput (1000 messages)", "info");

    let spawner = ThreadLocalActorSpawner::new();
    let count = Arc::new(Mutex::new(0));
    let count_clone = count.clone();

    let (actor_ref, _handle) = ThreadLocalFnActor::<u32>::start_fn(spawner, |mut ctx| async move {
        while let Some(_msg) = ctx.rx.next().await {
            *count_clone.lock().unwrap() += 1;
        }
    })
    .await
    .map_err(|e| format!("Failed to start actor: {:?}", e))?;

    // Send 1000 messages
    for i in 0..1000 {
        actor_ref.send_message(i).unwrap();
    }

    // Wait for processing
    wait_ms(500).await;

    let total = *count.lock().unwrap();
    if total != 1000 {
        return Err(format!("Expected 1000 messages, got {}", total));
    }

    log_to_page("✅ Successfully processed 1000 messages!", "success");
    Ok(())
}

// Helper to wait
async fn wait_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let window = window().expect("no window");
        window
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("set_timeout failed");
    });
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .expect("await failed");
}

// Run all tests
async fn run_all_tests() {
    log_to_page("========================================", "info");
    log_to_page("🚀 Starting WASM Thread-Local Actor Tests", "info");
    log_to_page("========================================", "info");

    set_status("⏳ Running tests...", "running");

    let mut all_passed = true;

    // Test 1
    match test_basic_message_passing().await {
        Ok(_) => {}
        Err(e) => {
            log_to_page(&format!("❌ Test 1 failed: {}", e), "error");
            all_passed = false;
        }
    }

    // Test 2
    match test_multiple_actors().await {
        Ok(_) => {}
        Err(e) => {
            log_to_page(&format!("❌ Test 2 failed: {}", e), "error");
            all_passed = false;
        }
    }

    // Test 3
    match test_string_messages().await {
        Ok(_) => {}
        Err(e) => {
            log_to_page(&format!("❌ Test 3 failed: {}", e), "error");
            all_passed = false;
        }
    }

    // Test 4
    match test_high_throughput().await {
        Ok(_) => {}
        Err(e) => {
            log_to_page(&format!("❌ Test 4 failed: {}", e), "error");
            all_passed = false;
        }
    }

    log_to_page("========================================", "info");
    if all_passed {
        log_to_page("🎉 All tests passed!", "success");
        set_status("✅ All tests passed!", "success");
    } else {
        log_to_page("⚠️ Some tests failed", "error");
        set_status("❌ Some tests failed", "error");
    }
    log_to_page("========================================", "info");
}

#[wasm_bindgen(start)]
pub fn main() {
    setup_panic_hook();
    console::log_1(&"WASM module loaded successfully!".into());

    render_main_page();
}

pub fn render_main_page() {
    let window = window().expect("no global window");
    let document = window.document().expect("no document");

    // Setup run button
    let run_button = document
        .get_element_by_id("runTests")
        .expect("run button not found");
    let closure = Closure::wrap(Box::new(move || {
        wasm_bindgen_futures::spawn_local(async {
            run_all_tests().await;
        });
    }) as Box<dyn FnMut()>);
    run_button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();

    // Setup clear button
    let clear_button = document
        .get_element_by_id("clearLogs")
        .expect("clear button not found");
    let closure = Closure::wrap(Box::new(move || {
        let win = web_sys::window().expect("no global window");
        let doc = win.document().expect("no document");
        let log_div = doc.get_element_by_id("log").expect("log div not found");
        log_div
            .set_inner_html("<div class='log-entry info'>Logs cleared. Ready for tests...</div>");
        set_status("Ready to run tests", "");
    }) as Box<dyn FnMut()>);
    clear_button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();

    // Setup Bluetooth button
    let bluetooth_button = document
        .get_element_by_id("bluetoothButton")
        .expect("bluetooth button not found");
    let closure = Closure::wrap(Box::new(move || {
        bluetooth_page::render_bluetooth_page();
    }) as Box<dyn FnMut()>);
    bluetooth_button
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .expect("add event listener");
    closure.forget();

    console::log_1(&"Event listeners attached. Click 'Run All Tests' to begin!".into());
}
