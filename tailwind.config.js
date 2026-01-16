/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{vue,js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        // 自定义深空色板
        cosmos: {
          950: '#05050A', // 黑洞视界 (背景极黑)
          900: '#0B0D17', // 深空底色
          800: '#151932', // 星云暗部
          700: '#1E2246', // 面板背景
          500: '#3D4C7A', // 弱边框
          300: '#8A9FD1', // 辅助文本
          100: '#E0E7FF', // 高亮文本
        },
        // 强调色
        starlight: {
          cyan: '#64FFDA',  // 科技蓝光
          purple: '#BD34FE', // 霓虹紫
          gold: '#FFD700',   // 恒星光芒
        }
      },
      backgroundImage: {
        'deep-void': 'linear-gradient(to bottom, #05050A, #0B0D17)',
      },
      fontFamily: {
        sans: ['Rajdhani', 'sans-serif'], // 正文用 Rajdhani (科技感细体)
        orbitron: ['Orbitron', 'sans-serif'], // 标题用 Orbitron (硬核科幻)
      }
    },
  },
  plugins: [],
}