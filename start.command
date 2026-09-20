#!/bin/bash
cd "$(dirname "$0")"

echo "💿 DiskFlow - 磁盘数据迁移与空间管理工具"
echo ""

# Check Rust
if ! command -v cargo &> /dev/null; then
  echo "❌ 未找到 Rust，正在安装..."
  echo "   请运行: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi
echo "🦀 Rust: $(rustc --version)"

# Check npm
if ! command -v npm &> /dev/null; then
  echo "❌ 未找到 npm，请先安装 Node.js"
  exit 1
fi
echo "📦 Node: $(node -v)"

# Install frontend deps
if [ ! -d "node_modules" ]; then
  echo "📥 安装前端依赖..."
  npm install
fi

# Check Tauri CLI
if ! npx tauri --version &> /dev/null; then
  echo "📥 安装 Tauri CLI..."
  npm install @tauri-apps/cli
fi

echo ""
echo "🚀 启动 DiskFlow..."
echo "   开发模式: npm run tauri dev"
echo "   打包: npm run tauri build"
echo ""
npm run tauri dev
