export interface Track {
    id: string; 
    title: string; 
    artist: string; 
    album: string; 
    cover: string; 
    duration: number; 
    path: string; 
    isAvailable?: boolean; 
  }
  
  export type PlayMode = 'sequence' | 'loop' | 'shuffle';
  
  export type NotificationCallback = (msg: string, type?: 'info' | 'error' | 'cooling') => void;