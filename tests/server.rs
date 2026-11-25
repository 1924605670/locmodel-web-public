use std::time::Duration;

#[test]
fn settings_and_folder_list_work_over_http() {
    let dir = tempfile::tempdir().unwrap();
    let base = locmodel::test_server(dir.path().to_path_buf());
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let mut settings = None;
    for _ in 0..50 {
        if let Ok(response) = client
            .post(format!("{base}/api/invoke"))
            .json(&serde_json::json!({"cmd":"get_settings","args":{}}))
            .send()
        {
            if response.status().is_success() {
                settings = Some(response.json::<serde_json::Value>().unwrap());
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let settings = settings.expect("服务没有在超时前响应");
    assert_eq!(settings["ok"], true);
    assert!(settings["value"]["models_dir"].is_string());

    let listed = client
        .post(format!("{base}/api/invoke"))
        .json(&serde_json::json!({"cmd":"fs_list","args":{"path":""}}))
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(listed["ok"], true);
    assert!(listed["value"]["entries"].as_array().unwrap().len() >= 3);

    let page = client.get(format!("{base}/")).send().unwrap();
    assert!(page.status().is_success());
    let html = page.text().unwrap();
    assert!(html.contains("web.js"));
    assert!(html.contains("LocModel"));
}
