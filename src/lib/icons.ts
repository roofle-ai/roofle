import {
  CircleCheck,
  CircleDot,
  CornerDownRight,
  createIcons,
  Download,
  MessageSquareQuote,
  Moon,
  Plus,
  Settings,
  Sparkles,
  Sun,
  Trash,
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
};

export function refreshIcons(): void {
  createIcons({ icons: ICONS });
}
