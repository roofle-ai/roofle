import {
  CircleCheck,
  CircleDot,
  ClipboardCheck,
  CornerDownRight,
  createIcons,
  Download,
  Hash,
  MessageSquareQuote,
  Moon,
  Plus,
  Rewind,
  Settings,
  Sparkles,
  Sun,
  Trash,
  WandSparkles,
} from "lucide";

const ICONS = {
  Plus,
  Settings,
  Moon,
  Sun,
  MessageSquareQuote,
  Sparkles,
  CircleDot,
  CircleCheck,
  CornerDownRight,
  Trash,
  Download,
  ClipboardCheck,
  WandSparkles,
  Rewind,
  Hash,
};

export function refreshIcons(): void {
  createIcons({ icons: ICONS });
}
