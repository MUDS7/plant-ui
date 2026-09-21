use aios_core::data_center::{ThreeDDatacenterRequest, ThreeDDatacenterResponse};
use anyhow::Context;
use plant_ui::data_publish::{DeletionCandidate, PublishCategory, PublishRequest};
use plant_ui::manual_data_publish::RoomCodePublishRequest;
use std::sync::OnceLock;
use std::time::Duration;

static BASE_URL: OnceLock<String> = OnceLock::new();

pub fn base_url() -> String {
    BASE_URL
        .get()
        .cloned()
        .or_else(|| std::env::var("PLANT_DATA_API_URL").ok())
        .unwrap_or_else(|| plant_ui::settings::DEFAULT_DATA_API_URL.into())
        .trim()
        .trim_end_matches('/')
        .to_owned()
}

pub fn set_base_url(base: String) -> anyhow::Result<()> {
    BASE_URL
        .set(base.trim_end_matches('/').to_owned())
        .map_err(|_| anyhow::anyhow!("数据服务地址已初始化，不能重复覆盖"))
}

/// 一次成功提交的服务端回执。
///
/// 数据中心成功响应中的 `LoginUrl`；未返回时保持为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitResult {
    pub message: String,
    pub login_url: Option<String>,
}

/// rs-server 数据中心接口：所有发布类别均复用 `ThreeDDatacenterRequest`。
/// 响应按 `ThreeDDatacenterResponse` 判断业务成功，而不只依赖 HTTP 状态。
pub async fn submit(base: &str, request: &PublishRequest) -> anyhow::Result<SubmitResult> {
    let body = request_body(request)?.to_string();
    let url = format!("{base}{}", request.category.endpoint());
    eprintln!("[data_publish] POST {url}\n[data_publish] request: {body}");
    let mut req = http_request(url, body);
    req.timeout = Some(Duration::from_secs(60));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .context("请求数据服务失败")?;
    let status = response.status;
    let body = response.text().context("数据服务响应不是 UTF-8")?;
    if !response.ok {
        anyhow::bail!("数据服务返回 HTTP {status}: {body}")
    }

    response_body(request.category, body)
}

pub async fn lookup_deletions(
    base: &str,
    request: &PublishRequest,
) -> anyhow::Result<Vec<DeletionCandidate>> {
    let payload = deletion_lookup_body(request)?.to_string();
    let url = format!("{base}/get_datacenter_delete_by_refnos");
    eprintln!(
        "[data_publish_delete_lookup] POST {url}\n[data_publish_delete_lookup] request: {payload}"
    );
    let mut req = http_request(url.clone(), payload.clone());
    req.timeout = Some(Duration::from_secs(60));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("POST {url} 请求失败；请求体：{payload}"))?;
    let status = response.status;
    let body = response
        .text()
        .with_context(|| format!("POST {url} 返回 HTTP {status}，响应不是 UTF-8"))?;
    if !response.ok {
        anyhow::bail!("POST {url} 返回 HTTP {status}；请求体：{payload}；响应体：{body}")
    }
    serde_json::from_str(body).with_context(|| {
        format!("POST {url} 返回 HTTP {status}，响应不是 id/name 数组；响应体：{body}")
    })
}

fn deletion_lookup_body(request: &PublishRequest) -> anyhow::Result<serde_json::Value> {
    if request.elements.is_empty() {
        anyhow::bail!("请至少添加一个元素");
    }
    Ok(serde_json::json!({
        "refnos": request.elements.iter().map(|element| {
            element.refno.to_string().replace('_', "/")
        }).collect::<Vec<_>>(),
    }))
}

pub async fn submit_room_codes(
    base: &str,
    request: &RoomCodePublishRequest,
) -> anyhow::Result<SubmitResult> {
    if request.room_codes.is_empty() {
        anyhow::bail!("请至少填写一条设备名称和房间号");
    }
    let body = serde_json::to_string(request)?;
    let url = format!("{base}/send_room_code_to_data_center");
    eprintln!("[room_code_publish] POST {url}\n[room_code_publish] request: {body}");
    let mut req = http_request(url, body);
    req.timeout = Some(Duration::from_secs(60));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .context("请求数据服务失败")?;
    let status = response.status;
    let body = response.text().context("数据服务响应不是 UTF-8")?;
    if !response.ok {
        anyhow::bail!("数据服务返回 HTTP {status}: {body}")
    }
    three_d_response_body(body)
}

fn http_request(url: String, body: String) -> ehttp::Request {
    ehttp::Request::new(
        ehttp::Method::POST,
        url,
        ehttp::Headers::new(&[("content-type", "application/json; charset=utf-8")]),
    )
    .with_body(body.into_bytes())
}

fn request_body(request: &PublishRequest) -> anyhow::Result<serde_json::Value> {
    if request.elements.is_empty() {
        anyhow::bail!("请至少添加一个元素");
    }
    let mut body = serde_json::to_value(ThreeDDatacenterRequest {
        refnos: request
            .elements
            .iter()
            .map(|element| element.refno.to_string())
            .collect(),
        title: request.title.clone(),
        create_rvm_relations: true,
        b_first_time_design: false,
    })?;
    if matches!(
        request.category,
        PublishCategory::Process
            | PublishCategory::Electrical
            | PublishCategory::Instrumentation
            | PublishCategory::Ventilation
    ) {
        body.as_object_mut()
            .expect("ThreeDDatacenterRequest 必须序列化为 JSON 对象")
            .insert(
                "delete_refnos".into(),
                serde_json::to_value(&request.delete_refnos)?,
            );
    }
    Ok(body)
}

fn response_body(_category: PublishCategory, body: &str) -> anyhow::Result<SubmitResult> {
    three_d_response_body(body)
}

fn three_d_response_body(body: &str) -> anyhow::Result<SubmitResult> {
    let response: ThreeDDatacenterResponse =
        serde_json::from_str(body).context("数据服务响应不符合 ThreeDDatacenterResponse 契约")?;
    if !response.success {
        anyhow::bail!(
            "{}",
            if response.result.is_empty() {
                "数据中心发布失败"
            } else {
                &response.result
            }
        );
    }

    Ok(SubmitResult {
        message: response.result,
        login_url: (!response.login_url.trim().is_empty()).then_some(response.login_url),
        //login_url: Some("http://pms.powerpms.net:1801/sysin.html".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_ui::{
        RefU64,
        data_publish::{DesignPhase, PublishElement},
    };

    fn request(category: PublishCategory) -> PublishRequest {
        PublishRequest {
            title: "发布测试".into(),
            category,
            design_phase: DesignPhase::Detailed,
            elements: vec![PublishElement {
                refno: RefU64::from(12_345_u64),
                name: "/PIPE-100".into(),
            }],
            delete_refnos: vec!["24383/66458".into(), "24383/66457".into()],
        }
    }

    #[test]
    fn four_professional_publish_endpoints_include_delete_refnos() {
        for category in [
            PublishCategory::Process,
            PublishCategory::Electrical,
            PublishCategory::Instrumentation,
            PublishCategory::Ventilation,
        ] {
            assert_eq!(
                request_body(&request(category)).unwrap(),
                serde_json::json!({
                "refnos": ["0_12345"],
                "title": "发布测试",
                "create_rvm_relations": true,
                "b_first_time_design": false,
                "delete_refnos": ["24383/66458", "24383/66457"],
                })
            );
        }
    }

    #[test]
    fn room_publish_uses_the_datacenter_contract() {
        assert_eq!(
            request_body(&request(PublishCategory::Room)).unwrap(),
            serde_json::json!({
                "refnos": ["0_12345"],
                "title": "发布测试",
                "create_rvm_relations": true,
                "b_first_time_design": false,
            })
        );
    }

    #[test]
    fn empty_publish_requests_are_rejected() {
        let mut request = request(PublishCategory::Process);
        request.elements.clear();
        assert!(request_body(&request).is_err());
    }

    #[test]
    fn deletion_lookup_uses_only_slash_separated_refnos() {
        let mut request = request(PublishCategory::Process);
        request.elements.push(PublishElement {
            refno: RefU64::from(12_346_u64),
            name: "/PIPE-101".into(),
        });
        assert_eq!(
            deletion_lookup_body(&request).unwrap(),
            serde_json::json!({"refnos": ["0/12345", "0/12346"]})
        );
    }

    #[test]
    fn deletion_lookup_response_selects_every_candidate() {
        let candidates: Vec<DeletionCandidate> =
            serde_json::from_str(r#"[{"id":"24383/66458","name":"/1WCC1135"}]"#).unwrap();
        assert_eq!(candidates[0].id, "24383/66458");
        assert_eq!(candidates[0].name, "/1WCC1135");
        assert!(candidates[0].selected);
    }

    #[test]
    fn publish_http_request_has_one_json_content_type() {
        let request = http_request("http://127.0.0.1:9099/get_gy_bran_data".into(), "{}".into());

        assert_eq!(request.method, ehttp::Method::POST);
        assert_eq!(
            request.headers.get_all("content-type").collect::<Vec<_>>(),
            vec!["application/json; charset=utf-8"]
        );
        assert_eq!(request.body, b"{}");
    }

    #[test]
    fn successful_responses_keep_the_server_login_url() {
        let result = response_body(
            PublishCategory::Process,
            r#"{"Success":true,"Result":"已提交","KeyValue":"","LoginUrl":"https://example.test/login"}"#,
        )
        .unwrap();

        assert_eq!(result.message, "已提交");
        assert_eq!(
            result.login_url.as_deref(),
            Some("https://example.test/login")
        );
    }

    #[test]
    fn room_responses_use_the_datacenter_contract() {
        let result = response_body(
            PublishCategory::Room,
            r#"{"Success":true,"Result":"已提交","KeyValue":"","LoginUrl":"https://example.test/login"}"#,
        )
        .unwrap();

        assert_eq!(result.message, "已提交");
        assert_eq!(
            result.login_url.as_deref(),
            Some("https://example.test/login")
        );
    }

    #[test]
    fn professional_response_rejects_business_failures() {
        let error = response_body(
            PublishCategory::Process,
            r#"{"Success":false,"Result":"发布被拒绝","KeyValue":"","LoginUrl":""}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("发布被拒绝"));
    }
}
