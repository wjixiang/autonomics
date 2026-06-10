/// Layers (in order)
/// 1. Agent identity: specify the role and identity of the agent
/// 2. SOP: specify the usages, examples of avaliable tools (when to use, how to use). NOT include schema of tools (directly pass to LlmClient)
#[derive(Default)]
pub struct SystemPromptBuilder {
    identity: String,
    tooluse_guidence: String,
    extra_section: String,
}
impl SystemPromptBuilder {
    pub fn with_extra_section(mut self, section: String) -> Self {
        self.extra_section = section;
        self
    }

    pub fn build_tooluse_guidence(mut self) -> Self {
        self.tooluse_guidence = concat!(
            "## 工具使用\n",
            "使用工具完成任务。当操作相互独立时，应该在单个回复中返回多个工具调用。例如，创建多个实体或链接多个孤立知识条目时，应在一个回复中一起发出所有工具调用，而不是每个调用一次。\n",
            "单个回复中的工具调用并行执行，这大大减少了往返时间。\n\n",
            "## 任务完成\n",
            "当任务全部完成后，**直接输出结束文字**即可——回复中没有任何工具调用即表示任务结束，不需要额外的终止动作。\n",
            "`attempt_complete` 工具已**废弃（遗留）**，新流程不再需要调用它；如果调用了也会被正常处理。\n",
        ).to_string();
        self
    }

    pub fn build_identity(mut self) -> Self {
        self.identity = "你是一位生物医学研究助手。".to_string();
        self
    }

    pub fn parse(self) -> String {
        let mut system_prompt = String::new();

        system_prompt.push_str(&self.identity);
        system_prompt.push('\n');
        if !self.extra_section.is_empty() {
            system_prompt.push_str(&self.extra_section);
            system_prompt.push('\n');
        }
        system_prompt.push_str(&self.tooluse_guidence);
        system_prompt.push('\n');

        system_prompt
    }
}
