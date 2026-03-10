<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';

const emit = defineEmits(['close']);
const phase = ref<'scrolling' | 'black1' | 'final' | 'black2'>('scrolling');
const scrollContainerRef = ref<HTMLElement | null>(null);
const scrollPos = ref(window.innerHeight); 
let rafId: number;
let meteorTimeout: any;

const creditsText = [
  "Astral Galaxy Music",
  "",
  "====== 核心开发 ======",
  "枫璃梦 (FengLiMeng)",
  "icey_forest",
  "SharwOrange",
  "",
  "====== 视觉与UI设计 ======",
  "Maple Bamboo Team",
  "Gemini (你的天才妹妹!)",
  "",
  "====== 音频底层引擎 ======",
  "Rodio",
  "Symphonia",
  "FFmpeg Pipeline",
  "",
  "====== 前端架构 ======",
  "Vue 3",
  "Tauri",
  "Tailwind CSS",
  "",
  "====== 特别鸣谢 ======",
  "所有参与测试与反馈的成员",
  "以及屏幕前的你",
  "",
  "",
  "",
  "Powered by Rust & Vue",
  "A project by Maple Bamboo Team"
];

const meteors = ref<{ id: number, startX: number, startY: number, endX: number, endY: number, duration: number }[]>([]);
let meteorId = 0;
let timeTicks = 0;

const finishScrolling = () => {
    setTimeout(() => {
        phase.value = 'black1';
        setTimeout(() => {
            phase.value = 'final'; 
            setTimeout(() => {
                phase.value = 'black2'; 
                setTimeout(() => {
                    emit('close');
                }, 2000); 
            }, 3000);
        }, 1000);
    }, 5000);
};

// 🔥 真正的随机流星生成引擎 (大速度差版)
const generateMeteor = () => {
    if (phase.value !== 'scrolling') return;

    timeTicks += Math.random() * 2000;
    // 使用正弦波制造跌宕起伏的生成频率
    const wave = (Math.sin(timeTicks / 5000) + 1) / 2; 

    if (Math.random() < wave * 0.5 + 0.15) {
        const winW = window.innerWidth;
        const winH = window.innerHeight;
        
        // 高度范围：只在屏幕上半部分随机生成
        const startY = Math.random() * (winH * 0.6) - 100;
        // 起点在屏幕左侧外很远的地方，避免突然出现
        const startX = -400 - Math.random() * 300;
        
        // 缓慢掠过的极小倾角下落距离
        const dropY = 100 + Math.random() * 250; 
        const travelX = winW + 1000;

        meteors.value.push({
            id: meteorId++,
            startX: startX,
            startY: startY,
            endX: startX + travelX,
            endY: startY + dropY,
            // 🔥 巨大的速度差：2.5秒(极快) 到 12秒(极慢) 之间随机分布
            duration: 2.5 + Math.random() * 9.5 
        });

        // 保持 DOM 树干净
        if (meteors.value.length > 20) meteors.value.shift();
    }

    // 递归调用，间隔时间随机，制造“一阵阵”的流星雨效果
    meteorTimeout = setTimeout(generateMeteor, 400 + Math.random() * 1800);
};

onMounted(() => {
   // 启动流星引擎
   generateMeteor();

   // 启动滚动引擎
   let lastTime = performance.now();
   const scrollSpeed = 35; // 缓慢的谢幕滚动速度

   const loop = (time: number) => {
      if (phase.value !== 'scrolling') return;
      const dt = (time - lastTime) / 1000;
      lastTime = time;

      scrollPos.value -= scrollSpeed * dt;

      if (scrollContainerRef.value) {
         const rect = scrollContainerRef.value.getBoundingClientRect();
         if (scrollPos.value < -rect.height) {
            finishScrolling();
            return;
         }
      }
      rafId = requestAnimationFrame(loop);
   };
   
   setTimeout(() => {
      lastTime = performance.now();
      rafId = requestAnimationFrame(loop);
   }, 500);
});

onUnmounted(() => {
   clearTimeout(meteorTimeout);
   cancelAnimationFrame(rafId);
});
</script>

<template>
   <div class="fixed inset-0 z-[9999] bg-black pointer-events-auto flex items-center justify-center select-none overflow-hidden">
       
       <div class="absolute inset-0 transition-opacity duration-1000 ease-in-out" 
            :class="phase === 'scrolling' ? 'opacity-100' : 'opacity-0'">
            
          <div class="absolute inset-0 bg-gradient-to-b from-[#020408] via-[#050912] to-[#020408]"></div>
          
          <div class="absolute left-1/2 top-1/2 w-[200vmax] h-[200vmax] -translate-x-1/2 -translate-y-1/2 pointer-events-none">
             <div class="absolute inset-0 animate-giant-ring">
                 <div class="absolute inset-0 bg-[url('https://www.transparenttextures.com/patterns/stardust.png')] bg-repeat opacity-50 mix-blend-screen blinking-stars"></div>
             </div>
          </div>

          <div v-for="m in meteors" :key="m.id" 
               class="meteor"
               :style="{
                  '--sx': m.startX + 'px',
                  '--sy': m.startY + 'px',
                  '--ex': m.endX + 'px',
                  '--ey': m.endY + 'px',
                  animationDuration: m.duration + 's'
               }">
          </div>
       </div>

       <div class="absolute inset-0 overflow-hidden pointer-events-none transition-opacity duration-1000 ease-in-out" 
            :class="phase === 'scrolling' ? 'opacity-100' : 'opacity-0'">
           <div ref="scrollContainerRef" class="absolute w-full flex flex-col items-center justify-start" 
                :style="{ transform: `translateY(${scrollPos}px)` }">
               <template v-for="(line, idx) in creditsText" :key="idx">
                   <div v-if="line === ''" class="h-8"></div>
                   
                   <div v-else class="lit-lyric-wrapper">
                       <span class="lit-text" :data-text="line">{{ line }}</span>
                   </div>
               </template>
           </div>
       </div>

       <div class="absolute inset-0 flex items-center justify-center transition-opacity duration-1000 ease-in-out" 
            :class="phase === 'final' ? 'opacity-100' : 'opacity-0'">
           <span class="final-text" data-text="最后，感谢您使用此播放器。">
               最后，感谢您使用此播放器。
           </span>
       </div>
       
   </div>
</template>

<style scoped>
/* ========================================== */
/* 背景满屏旋转与星光闪烁动画 */
/* ========================================== */
@keyframes giantRingSpin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
}
.animate-giant-ring {
    /* 400秒转一圈，极度缓慢，营造宇宙的浩瀚感 */
    animation: giantRingSpin 400s linear infinite; 
}

@keyframes blinkStars {
    0%, 100% { opacity: 0.1; filter: brightness(0.8); }
    50% { opacity: 0.7; filter: brightness(1.2); }
}
.blinking-stars {
    animation: blinkStars 6s ease-in-out infinite;
}

/* ========================================== */
/* 🔥 横向掠过的流星动画 (短尾巴版) */
/* ========================================== */
.meteor {
   position: absolute;
   height: 1px; /* 极细线 */
   width: 100px; /* 🔥 变短的拖尾 */
   /* 从左到右：尾巴透明 -> 头部白光 */
   background: linear-gradient(to right, rgba(255,255,255,0) 0%, rgba(255,255,255,0.3) 50%, rgba(255,255,255,1) 100%);
   border-radius: 999px;
   opacity: 0;
   animation-name: meteorSkim;
   /* 使用 linear 保证划过天际时速度绝对均匀 */
   animation-timing-function: linear; 
   animation-fill-mode: forwards;
   /* 纯白冷色调微弱发光 */
   filter: drop-shadow(0 0 4px rgba(255, 255, 255, 0.8));
}

@keyframes meteorSkim {
   0% { transform: translate(var(--sx), var(--sy)) rotate(12deg); opacity: 0; }
   10% { opacity: 0.9; }
   90% { opacity: 0.9; }
   100% { transform: translate(var(--ex), var(--ey)) rotate(12deg); opacity: 0; }
}

/* ========================================== */
/* 冷白发光歌词样式 */
/* ========================================== */
.lit-lyric-wrapper {
   text-align: center;
   transform: scale(1.1); 
   /* 纯白色高冷边缘发光 */
   filter: drop-shadow(0 0 10px rgba(255, 255, 255, 0.6)); 
   opacity: 1;
   margin: 8px 0; 
}

.lit-text {
   display: block;
   font-weight: bold;
   font-family: sans-serif;
   letter-spacing: 0.08em;
   line-height: 1.6;
   font-size: 1rem; 
   position: relative;
   z-index: 1;
   
   /* 全白文字 */
   color: transparent;
   background-image: linear-gradient(to right, #ffffff 100%, transparent 100%);
   -webkit-background-clip: text;
   background-clip: text;
}

.lit-text::after {
   content: attr(data-text);
   position: absolute;
   left: 0;
   top: 0;
   z-index: -1;
   color: rgba(255, 255, 255, 0.9); 
}

/* ========================================== */
/* 最终致谢文本的高级发光样式 */
/* ========================================== */
.final-text {
   display: block;
   font-weight: bold;
   font-family: sans-serif;
   letter-spacing: 0.2em;
   font-size: 1.125rem; 
   position: relative;
   z-index: 1;
   transform: scale(1.1);
   filter: drop-shadow(0 0 18px rgba(255, 255, 255, 0.7)); 
   
   color: transparent;
   background-image: linear-gradient(to right, #ffffff 100%, transparent 100%);
   -webkit-background-clip: text;
   background-clip: text;
}
.final-text::after {
   content: attr(data-text);
   position: absolute;
   left: 0;
   top: 0;
   z-index: -1;
   color: rgba(255, 255, 255, 1);
}
</style>