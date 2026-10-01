import {
  CircleCheck,
  CircleDot,
  ClipboardCheck,
  CornerDownRight,
  Cpu,
  createIcons,
  Download,
  Hash,
  MessageSquareQuote,
  Moon,
  PanelLeft,
  PanelRight,
  Plus,
  Rewind,
  Save,
  Settings,
  Sparkles,
  Sun,
  Trash,
  UserRound,
  WandSparkles
} from "lucide";

const ICONS = {
  Plus,
  Settings,
  Moon,
  PanelLeft,
  PanelRight,
  Save,
  Sun,
  MessageSquareQuote,
  Sparkles,
  CircleDot,
  CircleCheck,
  CornerDownRight,
  Trash,
  UserRound,
  Cpu,
  Download,
  ClipboardCheck,
  WandSparkles,
  Rewind,
  Hash,
};

export function refreshIcons(): void {
  createIcons({ icons: ICONS });
}
