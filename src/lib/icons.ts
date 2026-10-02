import {
  CircleCheck,
  CircleDot,
  ClipboardCheck,
  Clock,
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
  RotateCcw,
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
  Clock,
  CornerDownRight,
  Trash,
  UserRound,
  Cpu,
  Download,
  ClipboardCheck,
  WandSparkles,
  Rewind,
  RotateCcw,
  Hash,
};

export function refreshIcons(): void {
  createIcons({ icons: ICONS });
}
