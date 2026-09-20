//! UI 线程与数据线程（tokio）之间的桥。
//!
//! eframe 的绘制是同步的，而 SUL_DB 全是 async：这里起一个常驻数据线程，
//! 请求经 channel 进、结果经 channel 出，UI 每帧非阻塞地收。每个结果落地后
//! `request_repaint`，免得空闲中的 UI 睡过结果。

use std::sync::mpsc;

use plant_ui::data_publish::PublishRequest;
use plant_ui::manual_data_publish::RoomCodePublishRequest;
use plant_ui::model_update::{Enqueued, Preview, ProgressEvent};
use plant_ui::task_queue::Poll as QueuePoll;
use plant_ui_data::{EleTreeNode, RefU64};

pub enum Req {
    /// 懒加载某节点的直接子层。
    Children(RefU64),
    /// 选中元素的 UI 属性表。
    Props(RefU64),
    /// 加载这些模型树根下已经生成的几何实例。
    Models(Vec<RefU64>, bool),
    /// eye 显示路径：逐个解析树节点下已经生成的模型，不生成缺失模型。
    ModelScopes { epoch: u64, targets: Vec<RefU64> },
    /// 命令行按名称定位元素。
    ResolveName(String),
    /// 树定位目标的祖先链。目标不在已加载的树里时才发，见 ADR-0014。
    Ancestors(RefU64),
    /// 重跑启动序列。连库失败后命令行上的「重试」走这条，结果仍走 `Evt::Ready`。
    Reconnect,
    ModelUpdatePreview {
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    ModelUpdateExecute {
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 取回工作：丢缓存，重查 SITE 根层与这些已展开分支的子层。
    ///
    /// 分支由 App 侧算好交下来。数据线程不认识展开状态，也不该认识——它只是
    /// 「照这张单子重查一遍」。
    GetWork {
        branches: Vec<RefU64>,
        /// 数据已应用但模型仍在后台生成时为 false：刷新树与属性，保留当前三维。
        reload_models: bool,
    },
    /// 读一次设计库水位，给取回工作旁边那行提示用。
    PendingSessions,
    /// 队列面板的一次轮询（队列快照 + 任务表 + health + 持久欠账）。
    QueuePoll { base: String },
    /// 暂停 / 恢复出队。
    QueueSetPaused { base: String, paused: bool },
    DataPublish {
        base: String,
        request: PublishRequest,
    },
    LookupDataPublishDeletions {
        base: String,
        request: PublishRequest,
    },
    RoomCodePublish {
        base: String,
        request: RoomCodePublishRequest,
    },
}

/// 一次取回工作重新查回来的那部分树。
pub struct GetWork {
    pub sites: Vec<EleTreeNode>,
    pub reload_models: bool,
    /// 重查成功的分支及其新的直接子层。
    pub branches: Vec<(RefU64, Vec<EleTreeNode>)>,
    /// 重查失败的分支及原因。一个分支查不动不该让整次取回作废，
    /// 它那一层就保持原样，失败单独进日志。
    pub failed: Vec<(RefU64, String)>,
}

/// 启动序列（连接 + 工程标识 + SITE 根层）的合并产物。
pub struct ReadyInfo {
    pub project: String,
    /// 当前 MDB 名（带前导 `/`）。模型更新用它定本期执行范围。
    pub mdb: String,
    pub ns: String,
    pub db_nums: Vec<u32>,
    /// 开始读取根层的时刻。首次队列快照只补这之后完成的批次，避免启动期间漏刷新。
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub sites: Vec<EleTreeNode>,
}

pub enum Evt {
    Ready(anyhow::Result<ReadyInfo>),
    Children(RefU64, anyhow::Result<Vec<EleTreeNode>>),
    Props(RefU64, anyhow::Result<Vec<plant_ui_data::Attr>>),
    Models(bool, anyhow::Result<Vec<aios_core::GeomInstQuery>>),
    ModelScopeProgress {
        epoch: u64,
        target: RefU64,
        done: usize,
        total: usize,
    },
    ModelScope(u64, RefU64, anyhow::Result<Vec<aios_core::GeomInstQuery>>),
    ResolvedName(String, anyhow::Result<Option<RefU64>>),
    /// 目标的祖先链，「自己 -> 上级 -> …」序。带上请求时的那个 refno：
    /// 连续定位只算最后一次，晚到的旧链要认得出来才好丢。
    Ancestors(RefU64, anyhow::Result<Vec<RefU64>>),
    ModelUpdatePreview(anyhow::Result<Preview>),
    /// 「扫描 + 入队」的回执。合流之后它不再是单个 task_id。
    ModelUpdateExecute(anyhow::Result<Enqueued>),
    /// 取回工作的整批结果。根层查不动就是整次失败——根层没了树无从谈起。
    GetWork(anyhow::Result<GetWork>),
    /// 设计库水位。查不动就不显示那行提示，不值得为它报错。
    PendingSessions(anyhow::Result<u32>),
    /// 队列面板的一次轮询结果。四份数据一起换代，不留自相矛盾的中间态。
    QueuePoll(anyhow::Result<QueuePoll>),
    /// 暂停 / 恢复的回执。真值仍以下一次轮询的快照为准，这里只负责把失败说出来。
    QueueSetPaused(bool, anyhow::Result<()>),
    DataPublish(
        PublishRequest,
        anyhow::Result<crate::data_publish_api::SubmitResult>,
    ),
    DataPublishDeletions(anyhow::Result<Vec<plant_ui::data_publish::DeletionCandidate>>),
    RoomCodePublish(anyhow::Result<crate::data_publish_api::SubmitResult>),
    /// 队列视图的逐单元明细，带发生在哪个任务上。
    ///
    /// 这三个只有原生端的 WebSocket 构造（`model_update_ws`）；wasm 的 Feed 是
    /// 轮询兜底的桩，只发 `QueueFeedDown`，所以 wasm 目标上它们没有构造点。
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    QueueProgress(String, ProgressEvent),
    /// 有任务起讫。只当醒钟用：叫轮询早一拍去取，不拿它改行状态。
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    QueueTaskChanged,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    QueueFeedLive,
    QueueFeedDown(String),
}

pub struct Bridge {
    pub req: mpsc::Sender<Req>,
    pub evt: mpsc::Receiver<Evt>,
    /// 给数据线程之外的生产者用（现在只有那条 WebSocket）。UI 每帧只认一个收端，
    /// 长连接自己另开一条 channel 的话就得在 `pump_events` 里再轮询一次。
    pub evt_tx: mpsc::Sender<Evt>,
}

enum ModelLoad {
    Replace(Vec<RefU64>, bool),
    Scopes(u64, Vec<RefU64>),
}

fn route_model_load(req: Req, tx: &mpsc::Sender<ModelLoad>) -> Result<Option<Req>, ModelLoad> {
    match req {
        Req::Models(roots, debt_reload) => tx
            .send(ModelLoad::Replace(roots, debt_reload))
            .map(|_| None)
            .map_err(|error| error.0),
        Req::ModelScopes { epoch, targets } => tx
            .send(ModelLoad::Scopes(epoch, targets))
            .map(|_| None)
            .map_err(|error| error.0),
        req => Ok(Some(req)),
    }
}

/// 取回工作：先丢本进程的查询缓存，再把根层与这些分支重查一遍。
///
/// 缓存必须先丢。重查 SITE 根层走的是带 memoize 的那条查询，缓存还在的话它会
/// 原样把旧的那份还回来，界面看着刷新过了、内容一个字没变。
async fn get_work(branches: &[RefU64], reload_models: bool) -> anyhow::Result<GetWork> {
    plant_ui_data::invalidate_all().await;
    let sites = plant_ui_data::site_nodes().await?;
    let mut loaded = Vec::with_capacity(branches.len());
    let mut failed = Vec::new();
    for refno in branches {
        match plant_ui_data::child_nodes((*refno).into()).await {
            Ok(kids) => loaded.push((*refno, kids)),
            Err(error) => failed.push((*refno, crate::logs::error_chain(&error))),
        }
    }
    Ok(GetWork {
        sites,
        reload_models,
        branches: loaded,
        failed,
    })
}

/// 启动序列：连库、抓工程标识、抓 SITE 根层。三步任一失败都算没连上。
async fn ready() -> anyhow::Result<ReadyInfo> {
    plant_ui_data::connect().await?;
    let (project, mdb, ns, db_nums) = plant_ui_data::project_identity().await?;
    let observed_at = chrono::Utc::now();
    let sites = plant_ui_data::site_nodes().await?;
    Ok(ReadyInfo {
        project,
        mdb,
        ns,
        db_nums,
        observed_at,
        sites,
    })
}

/// 起数据线程：连库、抓工程标识与 SITE 根层，然后循环处理懒加载请求。
pub fn spawn(ctx: egui::Context, tasks: &bevy_wasm_tasks::Tasks<'_>) -> Bridge {
    let (req_tx, req_rx) = mpsc::channel();
    let (model_tx, model_rx) = mpsc::channel::<ModelLoad>();
    let (evt_tx, evt_rx) = mpsc::channel();
    let evt_tx_out = evt_tx.clone();
    let model_evt_tx = evt_tx.clone();
    let model_ctx = ctx.clone();
    let model_worker = move |mut task_ctx: bevy_wasm_tasks::TaskContext| async move {
        loop {
            while let Ok(load) = model_rx.try_recv() {
                match load {
                    ModelLoad::Replace(roots, debt_reload) => {
                        let result = plant_ui_data::model_instances(&roots).await;
                        let _ = model_evt_tx.send(Evt::Models(debt_reload, result));
                        model_ctx.request_repaint();
                    }
                    ModelLoad::Scopes(epoch, targets) => {
                        for target in targets {
                            let progress_tx = model_evt_tx.clone();
                            let result = plant_ui_data::model_instances_with_progress(
                                &[target],
                                |done, total| {
                                    let _ = progress_tx.send(Evt::ModelScopeProgress {
                                        epoch,
                                        target,
                                        done,
                                        total,
                                    });
                                    model_ctx.request_repaint();
                                },
                            )
                            .await;
                            let _ = model_evt_tx.send(Evt::ModelScope(epoch, target, result));
                            model_ctx.request_repaint();
                        }
                    }
                }
            }
            task_ctx.sleep_updates(1).await;
        }
    };
    let worker = move |mut task_ctx: bevy_wasm_tasks::TaskContext| async move {
        let _ = evt_tx.send(Evt::Ready(ready().await));
        ctx.request_repaint();

        loop {
            while let Ok(req) = req_rx.try_recv() {
                // 模型实例冷加载在大库上可达 88 秒。它若占住这个串行桥，树展开和
                // 属性查询都会排在它后面，界面看上去像节点打不开。PC 与 Web 都把
                // 它送到专用串行任务，交互查询留在这里立即处理。
                let req = match route_model_load(req, &model_tx) {
                    Ok(Some(req)) => req,
                    Ok(None) => continue,
                    Err(ModelLoad::Replace(_, debt_reload)) => {
                        let _ = evt_tx.send(Evt::Models(
                            debt_reload,
                            Err(anyhow::anyhow!("模型查询任务已停止")),
                        ));
                        ctx.request_repaint();
                        continue;
                    }
                    Err(ModelLoad::Scopes(epoch, targets)) => {
                        for target in targets {
                            let _ = evt_tx.send(Evt::ModelScope(
                                epoch,
                                target,
                                Err(anyhow::anyhow!("模型查询任务已停止")),
                            ));
                        }
                        ctx.request_repaint();
                        continue;
                    }
                };
                match req {
                    Req::Reconnect => {
                        let _ = evt_tx.send(Evt::Ready(ready().await));
                        ctx.request_repaint();
                    }
                    Req::Children(refno) => {
                        let r = plant_ui_data::child_nodes(refno.into()).await;
                        let _ = evt_tx.send(Evt::Children(refno, r));
                        ctx.request_repaint();
                    }
                    Req::Props(refno) => {
                        let r = plant_ui_data::element_props(refno.into()).await;
                        let _ = evt_tx.send(Evt::Props(refno, r));
                        ctx.request_repaint();
                    }
                    Req::Models(..) | Req::ModelScopes { .. } => {
                        unreachable!("模型请求已路由到专用任务")
                    }
                    Req::ResolveName(name) => {
                        let result = plant_ui_data::resolve_name(&name).await;
                        let _ = evt_tx.send(Evt::ResolvedName(name, result));
                        ctx.request_repaint();
                    }
                    Req::Ancestors(refno) => {
                        let result = plant_ui_data::ancestor_refnos(refno.into()).await;
                        let _ = evt_tx.send(Evt::Ancestors(refno, result));
                        ctx.request_repaint();
                    }
                    Req::ModelUpdatePreview {
                        base,
                        project,
                        mdb,
                        namespace,
                    } => {
                        let result =
                            crate::model_update_api::preview(&base, &project, &mdb, &namespace)
                                .await;
                        let _ = evt_tx.send(Evt::ModelUpdatePreview(result));
                        ctx.request_repaint();
                    }
                    Req::ModelUpdateExecute {
                        base,
                        project,
                        mdb,
                        namespace,
                    } => {
                        let result =
                            crate::model_update_api::execute(&base, &project, &mdb, &namespace)
                                .await;
                        let _ = evt_tx.send(Evt::ModelUpdateExecute(result));
                        ctx.request_repaint();
                    }
                    Req::GetWork {
                        branches,
                        reload_models,
                    } => {
                        let result = get_work(&branches, reload_models).await;
                        let _ = evt_tx.send(Evt::GetWork(result));
                        ctx.request_repaint();
                    }
                    Req::PendingSessions => {
                        let result = plant_ui_data::pending_sessions().await;
                        let _ = evt_tx.send(Evt::PendingSessions(result));
                        ctx.request_repaint();
                    }
                    Req::QueuePoll { base } => {
                        let result = crate::model_update_api::poll_queue(&base).await;
                        let _ = evt_tx.send(Evt::QueuePoll(result));
                        ctx.request_repaint();
                    }
                    Req::QueueSetPaused { base, paused } => {
                        let result = crate::model_update_api::set_queue_paused(&base, paused).await;
                        let _ = evt_tx.send(Evt::QueueSetPaused(paused, result));
                        ctx.request_repaint();
                    }
                    Req::DataPublish { base, request } => {
                        let result = crate::data_publish_api::submit(&base, &request).await;
                        let _ = evt_tx.send(Evt::DataPublish(request, result));
                        ctx.request_repaint();
                    }
                    Req::LookupDataPublishDeletions { base, request } => {
                        let result =
                            crate::data_publish_api::lookup_deletions(&base, &request).await;
                        let _ = evt_tx.send(Evt::DataPublishDeletions(result));
                        ctx.request_repaint();
                    }
                    Req::RoomCodePublish { base, request } => {
                        let result =
                            crate::data_publish_api::submit_room_codes(&base, &request).await;
                        let _ = evt_tx.send(Evt::RoomCodePublish(result));
                        ctx.request_repaint();
                    }
                }
            }
            task_ctx.sleep_updates(1).await;
        }
    };
    #[cfg(target_arch = "wasm32")]
    {
        tasks.spawn_wasm(model_worker);
        tasks.spawn_wasm(worker);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        tasks.spawn_tokio(model_worker);
        tasks.spawn_tokio(worker);
    }
    Bridge {
        req: req_tx,
        evt: evt_rx,
        evt_tx: evt_tx_out,
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelLoad, Req, route_model_load};
    use plant_ui::RefU64;
    use std::sync::mpsc;

    #[test]
    fn model_load_uses_dedicated_lane() {
        let (model_tx, model_rx) = mpsc::channel();
        assert!(matches!(
            route_model_load(Req::Models(vec![RefU64::default()], false), &model_tx),
            Ok(None)
        ));
        assert!(matches!(
            model_rx.try_recv(),
            Ok(ModelLoad::Replace(roots, false)) if roots == vec![RefU64::default()]
        ));
        let target = RefU64::from(42);
        assert!(matches!(
            route_model_load(
                Req::ModelScopes {
                    epoch: 7,
                    targets: vec![target],
                },
                &model_tx
            ),
            Ok(None)
        ));
        assert!(matches!(
            model_rx.try_recv(),
            Ok(ModelLoad::Scopes(7, targets)) if targets == vec![target]
        ));
        assert!(matches!(
            route_model_load(Req::Props(RefU64::default()), &model_tx),
            Ok(Some(Req::Props(_)))
        ));
        assert!(matches!(
            route_model_load(Req::Children(RefU64::default()), &model_tx),
            Ok(Some(Req::Children(_)))
        ));

        drop(model_rx);
        assert!(matches!(
            route_model_load(
                Req::ModelScopes {
                    epoch: 8,
                    targets: vec![target],
                },
                &model_tx
            ),
            Err(ModelLoad::Scopes(8, targets)) if targets == vec![target]
        ));
    }
}
