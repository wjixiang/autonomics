#!/bin/bash
# integration_test.sh — 启动 server + 用 grpcurl 测试全流程

set -e

SKILL_DIR="/tmp/skill-test-$$"
PROTO_DIR="/mnt/disk3/agentik/crates/agentik-skill-proto/proto"
SERVER_ADDR="127.0.0.1:50051"
GRPCURL="$HOME/go/bin/grpcurl -plaintext -import-path $PROTO_DIR -proto skill_registry.proto"

# 1. 创建 fixture skills
mkdir -p "$SKILL_DIR/commit" "$SKILL_DIR/review"

cat > "$SKILL_DIR/commit/SKILL.md" << 'EOF'
---
name: commit
description: "Stage and commit changes"
aliases: [ci]
allowed_tools: [bash, read]
---
Commit all changes.
EOF

cat > "$SKILL_DIR/review/SKILL.md" << 'EOF'
---
name: review
description: "Code review"
allowed_tools: [bash, read, grep]
user_invocable: true
---
Review code.
EOF

# 2. 启动 server（后台）
cargo run -p agentik-skill-server -- \
  --addr "$SERVER_ADDR" --skill-dir "$SKILL_DIR" &
SERVER_PID=$!
sleep 3

# 3. 测试
echo "=== ListSkills ==="
$GRPCURL -d '{}' "$SERVER_ADDR" agentik.skill.v1.SkillRegistryService/ListSkills

echo ""
echo "=== GetSkill (commit) ==="
$GRPCURL -d '{"name": "commit"}' "$SERVER_ADDR" agentik.skill.v1.SkillRegistryService/GetSkill

echo ""
echo "=== GetSkill (alias: ci) ==="
$GRPCURL -d '{"name": "ci"}' "$SERVER_ADDR" agentik.skill.v1.SkillRegistryService/GetSkill

echo ""
echo "=== GetSkill (not found) ==="
$GRPCURL -d '{"name": "nope"}' "$SERVER_ADDR" agentik.skill.v1.SkillRegistryService/GetSkill

echo ""
echo "=== ReloadSkill ==="
$GRPCURL -d '{"name": "commit"}' "$SERVER_ADDR" agentik.skill.v1.SkillRegistryService/ReloadSkill

echo ""
echo "=== All tests passed ==="

# 4. 清理
kill $SERVER_PID 2>/dev/null
rm -rf "$SKILL_DIR"
