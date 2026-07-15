/// Visual test for ChatWidget rendering.
///
/// Run with:
///   cargo run --example chat_render
///
/// Displays a full-screen ratatui TUI with sample messages covering all
/// ChatLine variants and markdown features (headings, code blocks, tables,
/// bold/italic, blockquotes, links). Press `q` or `Ctrl+C` to quit.
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Frame, prelude::*, widgets::Paragraph};

use tui::state::{ChatLine, TurnUsage};
use tui::widgets::chat_widget::{ChatWidget, ChatWidgetState};

/// Sample messages that exercise every rendering path.
fn sample_messages() -> Vec<ChatLine> {
    vec![
        ChatLine::User("帮我分析一下这段 RNA-seq 数据的质量".into()),
        ChatLine::Thinking(
            "用户想对 RNA-seq 数据做质控分析，我需要先检查 fastq 文件是否存在...\n\
             然后运行 FastQC 工具..."
                .into(),
        ),
        ChatLine::Assistant {
            text: "\
# RNA-seq 质控报告

这是一份**基础的** RNA-seq 数据质量报告，以下是关键指标：

## 1. 总体统计

| 指标 | 值 | 状态 |
| :--- | :--- | :--- |
| Total Reads | 42,156,789 | ✅ |
| Q30 Rate | 94.2% | ✅ |
| Adapter Rate | 0.3% | ✅ |
| Duplication | 12.8% | ⚠️ |

## 2. GC 分布

GC 含量在 **45-55%** 之间，符合预期。

> 提示：如果 GC 分布出现双峰，可能是样本污染。

### 引用

更多工具请参考 [FastQC 官方文档](https://www.bioinformatics.babraham.ac.uk/projects/fastqc/)。

```bash
fastqc sample_R1.fastq.gz sample_R2.fastq.gz -o results/
multiqc results/ -o results/
```

常规文本混合 *斜体* 和 **粗体**。"
                .into(),
            usage: Some(TurnUsage {
                input_tokens: Some(1200),
                output_tokens: 340,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(800),
            }),
        },
        ChatLine::ToolCall {
            name: "bash".into(),
            input: "fastqc sample_R1.fastq.gz -o results/".into(),
        },
        ChatLine::ToolBackground {
            id: "tool-001".into(),
            name: "fastqc".into(),
        },
        ChatLine::ToolResult {
            ok: true,
            content: "FastQC complete. Report saved to results/sample_R1_fastqc.html".into(),
        },
        ChatLine::Error("连接远程服务器超时，请检查网络设置".into()),
        ChatLine::Separator,
        ChatLine::Assistant {
            text: "\
# 第二轮分析

以下是一个**宽表格**测试（列宽自适应）：

| Sample Name | Tissue | Replicate | Total Reads | Mapped Rate | RPKM |
| :--- | :--- | :--- | :--- | :--- | :--- |
| SRR001 | Liver | Rep1 | 38,241,556 | 96.3% | 12,450 |
| SRR002 | Liver | Rep2 | 41,002,338 | 95.8% | 13,210 |
| SRR003 | Brain | Rep1 | 29,873,122 | 94.1% | 8,930 |
| SRR004 | Brain | Rep2 | 31,445,891 | 94.5% | 9,102 |
| SRR005 | Heart | Rep1 | 35,678,012 | 95.2% | 11,340 |
| SRR006 | Kidney | Rep1 | 27,890,445 | 93.7% | 7,650 |

数据看起来不错，mapping rate 都在 93% 以上。"
                .into(),
            usage: Some(TurnUsage {
                input_tokens: Some(2100),
                output_tokens: 580,
                cache_creation_input_tokens: Some(1500),
                cache_read_input_tokens: None,
            }),
        },
        ChatLine::User("这些数据可以用 DESeq2 做差异表达分析吗？".into()),
        ChatLine::Assistant {
            text: "可以的，DESeq2 是做 RNA-seq **差异表达分析**的经典 R 包。\n\n\
                   不过请确保你的 count matrix 已经准备好了，格式通常是 `genes × samples` 的整数矩阵。\n\n\
                   ```R\nlibrary(DESeq2)\ncts <- read.csv('counts.csv', row.names=1)\ncoldata <- read.csv('coldata.csv')\ndds <- DESeqDataSetFromMatrix(countData=cts, colData=coldata, design=~ condition)\ndds <- DESeq(dds)\nres <- results(dds)\n```"
                .into(),
            usage: Some(TurnUsage {
                input_tokens: Some(800),
                output_tokens: 220,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(600),
            }),
        },
        ChatLine::User("给我画一个 RNA-seq 差异分析 pipeline 的流程图".into()),
        ChatLine::Assistant {
            text: "下面是这个 pipeline 的流程概览：\n\n\
                   ```text\n\
                   1. Raw FASTQ\n\
                   2. FastQC (质量检查)\n\
                   3. Trim Galore (接头去除)\n\
                   4. STAR (基因组映射)\n\
                   5. featureCounts (定量)\n\
                   6. DESeq2 (差异表达分析)\n\
                   7. 差异基因列表\n\
                   ```\n\n\
                   关键节点是质量控制这一步,如果 FastQC 不过关就需要重新测序。"
                .into(),
            usage: Some(TurnUsage {
                input_tokens: Some(1500),
                output_tokens: 420,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(900),
            }),
        },
    ]
}

fn main() -> color_eyre::Result<()> {
    // ── Terminal setup ──────────────────────────────────────────
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

    // ── State ────────────────────────────────────────────────────
    let messages = sample_messages();
    let mut widget_state = ChatWidgetState::new(0);

    // ── Event loop ────────────────────────────────────────────────
    loop {
        terminal.draw(|f| ui(f, &messages, &mut widget_state))?;

        if let Event::Key(key) = event::read()? {
            match key.kind {
                KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Down | KeyCode::Char('j') => {
                        widget_state.scroll_offset = widget_state.scroll_offset.saturating_add(1)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        widget_state.scroll_offset = widget_state.scroll_offset.saturating_sub(1)
                    }
                    KeyCode::PageDown => {
                        widget_state.scroll_offset = widget_state
                            .scroll_offset
                            .saturating_add(widget_state.viewport_height.into());
                    }
                    KeyCode::PageUp => {
                        widget_state.scroll_offset = widget_state
                            .scroll_offset
                            .saturating_sub(widget_state.viewport_height.into());
                    }
                    KeyCode::Home => widget_state.scroll_offset = 0,
                    KeyCode::End => {
                        widget_state.scroll_offset = widget_state
                            .total_lines
                            .saturating_sub(widget_state.viewport_height.into());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    // ── Restore terminal ─────────────────────────────────────────
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn ui(f: &mut Frame, messages: &[ChatLine], state: &mut ChatWidgetState) {
    // Full-screen chat area.
    let area = f.area();

    // Bottom status hint.
    let hint = "↑↓/j/k: scroll  PgUp/PgDn: page  Home/End: jump  q: quit";
    let hint_height = 1;
    let chat_area = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(hint_height),
    );

    ChatWidget {
        messages,
        cached_lines: None,
    }
    .render(chat_area, f.buffer_mut(), state);

    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
        Rect::new(
            area.x,
            area.y + area.height.saturating_sub(hint_height),
            area.width,
            hint_height,
        ),
    );
}
