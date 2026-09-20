//! plant-ui：绘制层（组件层 + 各屏纯绘制函数 + Vm 定义）。
//! 不认识数据库、不认识 Bevy：输入 &Vm，输出 Vec<Cmd>。

pub mod data_publish;
pub mod fonts;
pub mod manual_data_publish;
pub mod model_update;
pub mod project_picker;
pub mod settings;
pub mod style;
pub mod task_queue;
pub mod vm;
pub mod workbench;

pub use aios_core::RefU64;

/// 对模型本身的动作。模型树右键菜单与三维视口工具栏是同一件事的两个入口，
/// 共用这一个枚举，宿主只需映射一次。
///
/// 动作自带 `refno`：右键菜单打开后选中还可能被别处改掉，回头读 `vm.selection`
/// 就会作用到另一个元素上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelAction {
    /// 可见性作用于一整批：「隐藏这几个」是选中之后最常做的一件事，
    /// 拆成 N 条命令会在宿主侧变成 N 次深度展开与 N 次事件写入。
    SetVisible {
        refnos: Vec<aios_core::RefU64>,
        visible: bool,
    },
    /// 隐藏当前已显示的全部模型。全局动作，只在工具栏出现。
    HideAll,
    /// 把已加载的模型全部显示回来，`HideAll` 的对偶。
    ///
    /// 「全部」只到**已加载**为止：没加载过的元素在三维里根本没有实体，这条命令
    /// 不会去替用户加载它们，模型树上那些行也照旧停在「未加载」。加载是 `SetVisible`
    /// 那条路上的事，两者别混。
    ///
    /// 它会把用户在 `HideAll` **之前**亲手隐藏的那几个也一并显示出来——`HideAll`
    /// 已经把「谁是我自己藏的」这件事抹平了，恢复时分不出来，也不该假装分得出来。
    ShowAll,
    /// 相机对准某个元素——**只作用于主选中，不收整批**。
    ///
    /// 给一组元素取景要合并它们的包围盒，而宿主侧的 `FocusModelEvent` 只认单个
    /// refno。塞一组进去的结果是定位到其中某一个、还说不清是哪一个。定位本来
    /// 就是「带我到它跟前」，作用于主选中是说得清的语义。
    Focus(aios_core::RefU64),
    /// 相机拉远到覆盖当前所有可见模型。
    FitAll,
    /// 进入 / 退出距离测量。
    ToggleMeasure,
}

/// 三维视口里的一次相机运动。键位见 `view3d::camera`：左键 / 中键平移、
/// 右键旋转、滚轮缩放；未形成拖拽的左键点击仍用于拾取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMotion {
    Orbit,
    Pan,
}

/// 视口上的相机手势。
///
/// 视口画的是一张离屏渲染纹理，宿主的相机要的却是**渲染目标像素空间**的量；
/// 两者之间隔着一次等比裁切铺满（`view3d::cover_uv`），换算只有绘制层做得了，
/// 所以这里交出去的已经是折算好的值，宿主拿来直接喂给相机。
///
/// `Drag` 与 `Zoom` 在宿主侧都是「没有运动就不生效」，因此起手被挡下的手势
/// （比如从工具栏上按下去再拖）不会漏出半截运动，调用点不必自己记状态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraGesture {
    /// 按下：开始一次旋转或平移。`anchor` 是按下点在渲染纹理上的归一化 UV，
    /// 宿主拿它求转心——转心落在指针底下那个东西上，是这类相机的立身之本。
    Begin {
        motion: CameraMotion,
        anchor: [f32; 2],
    },
    /// 拖拽增量，单位是渲染目标的像素。
    Drag([f32; 2]),
    /// 滚轮缩放。`amount` 是 egui 的滚动点数，宿主折算成自己的量纲；
    /// `anchor` 同 `Begin`，缩放要朝着指针指的方向去。
    Zoom { amount: f32, anchor: [f32; 2] },
    /// 松手。之后的惯性归宿主的控制器管，绘制层不模拟。
    End,
}

/// UI 发出的命令：独立应用直接执行，接进 Bevy 后转成 Event。
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    SelectElement(aios_core::RefU64),
    /// 模型树上的选择集变更（Ctrl 加减 / Shift 区间）。
    ///
    /// 绘制层算好**结果**整体交出，而不是发「加了谁减了谁」：区间要按可见行序算，
    /// 那个序只有绘制层手上的 rows 才有，宿主复原不出来。
    SetSelection(vm::Selection),
    /// 定位到元素：选中它、把树上挡着的祖先一路展开、再把行滚进视野。
    /// 与 `SelectElement` 分开，是因为树上点行本来就看得见，不该跟着跳。
    LocateElement(aios_core::RefU64),
    /// 展开 / 折叠模型树节点；子层未加载时由 App 侧懒加载。
    ToggleExpand(aios_core::RefU64),
    /// 属性面板提交了一次改值（回车或失焦）。`attr` 是真实属性名，
    /// `value` 是已按类型校验过的显示串。
    EditAttr {
        refno: aios_core::RefU64,
        attr: String,
        value: String,
    },
    /// 命令行错误行的处置入口：重做该行对应的操作（App 侧按行号找回原请求）。
    RetryLog(u64),
    /// 面板错误态上的「重试」。命令行按行号回指得到操作，面板上没有行号，
    /// 所以直接说重做哪件事——一块面板的失败只有一种来源，说得出来。
    Reconnect,
    RetryProps(aios_core::RefU64),
    /// 清空日志缓冲。
    ClearLogs,
    /// 提交一条命令；解析与执行由宿主负责。
    SubmitCommand(String),
    /// 打开项目选择窗口。
    OpenProjectPicker,
    /// 使用宿主提供的配置加载一个项目。
    LoadProject(String),
    /// 取回工作：把库里此刻的样子取到界面上来——丢掉本端缓存、重查已展开的
    /// 树分支、重载已显示的模型。
    ///
    /// **它不触发后端增量更新。** 数据是别人写进去的还是自动同步追进来的，
    /// 这一端都不管；手动发起一轮增量是「模型更新」那个三步窗的事，两者
    /// 不合并——合并之后自动同步开着时这个菜单会被服务端直接拒掉。
    ///
    /// 不带 refno：变化明细本端拿不到（自动同步只广播摘要），所以刷新范围只能
    /// 是「当前看得见的那些」，由处置方按自己的展开状态与已加载集合算出来。
    GetWork,
    /// 打开设置任务窗。
    OpenSettings,
    /// 打开三维数据发布任务窗。
    OpenDataPublish,
    OpenManualDataPublish,
    SubmitRoomCodePublish(manual_data_publish::RoomCodePublishRequest),
    /// 提交三维数据发布请求；实际发送由宿主负责。
    SubmitDataPublish(data_publish::PublishRequest),
    LookupDataPublishDeletions(data_publish::PublishRequest),
    /// 打开并刷新模型增量更新预览。
    OpenModelUpdate,
    /// 重新读取当前项目的增量范围。
    RefreshModelUpdate,
    /// 放弃等待预览结果。**只放弃客户端这一侧**——服务端没有 cancel 接口，
    /// 扫描照跑，界面上也不许暗示它会停。
    CancelModelUpdatePreview,
    /// 执行当前项目的全部待更新库；范围由服务端的 Committed Watermark 决定。
    ExecuteModelUpdate,
    /// 任务队列上的「立刻扫一遍」。它**不插队**，作用只是别等服务端下一个 30 秒轮询。
    ScanNow,
    /// 暂停 / 恢复队列出队。暂停**只挡出队**，正在跑的那一批会跑完为止——
    /// 服务端没有中止接口，所以这条命令也不该被当成「停下来」。
    SetQueuePaused(bool),
    /// 重开队列视图的明细长连接。断线降级的是明细区，队列行走轮询不受影响。
    ReconnectQueueFeed,
    /// 点击三维视口；坐标是离屏渲染纹理的归一化 UV。
    /// 测量态下宿主把它当取点，否则当模型拾取——判断哪一种归宿主。
    PickViewport([f32; 2]),
    /// 三维视口上的相机操作。与 `PickViewport` 分开：点是一次性的语义动作，
    /// 而相机是一段连续运动，两者的生命周期对不上。
    Camera(CameraGesture),
    /// 离屏渲染目标该开多大，单位是**物理像素**。
    ///
    /// 视口这块矩形跟着 dock 分隔条走，只有绘制层手上有这个数，宿主复原不出来。
    /// 报物理像素而不是逻辑点：150% 缩放下按点数开纹理，铺到屏幕上就是一次放大
    /// 插值，管子边缘会糊。
    ///
    /// 尺寸对不上的每一帧都会发一遍——绘制层没有状态，记不住自己提过。**防抖归
    /// 宿主**：拖分隔条时这条命令每帧都在变，每帧重建一张几兆的纹理会把拖动拖成
    /// 幻灯片。
    ResizeViewport([u32; 2]),
    /// 对模型的显示 / 定位类动作。
    Model(ModelAction),
    /// 主题下发的三维视口配色：渐变背景上下两色与地面网格线色。
    ///
    /// 背景渐变画在宿主的全屏背景面片上（拷问定案第 2 题，用户点名 Bevy 内画），
    /// 而主题只有绘制层认识，所以切主题的当帧由 App 把三个颜色送下去；宿主无状态，
    /// 不认识「主题」，只认颜色。
    SetViewportBackground {
        top: egui::Color32,
        bottom: egui::Color32,
        grid: egui::Color32,
    },
    /// ViewCube 触发的一次视角跳转。
    ///
    /// `forward` / `up` 是**宿主世界系**（场景已含 PDMS Z-up → Bevy Y-up 那次旋转）
    /// 的单位向量——PDMS 语义到世界系的换算只有绘制层的立方体模块知道，宿主拿来
    /// 直接用。`fit = true`（Home 键）时同时把距离拉到能装下全部可见模型。
    /// 跳转带 0.3s 插值动画，动画期间来任何相机手势立即让位。
    SnapView {
        forward: [f32; 3],
        up: [f32; 3],
        fit: bool,
    },
}
