import type { ComponentType, SVGProps } from 'react'
import { createElement } from 'react'
import {
  PlusCircleIcon,
  PlusIcon,
  BookOpenIcon,
  ArchiveBoxIcon,
  ChevronDownIcon,
  ArrowLeftIcon,
  ArrowRightIcon,
  ArrowUpIcon,
  CheckIcon,
  ClipboardDocumentListIcon,
  ClockIcon,
  XCircleIcon,
  CodeBracketIcon,
  CommandLineIcon,
  DocumentDuplicateIcon,
  ClipboardDocumentCheckIcon,
  CpuChipIcon,
  ExclamationTriangleIcon,
  DocumentIcon,
  DocumentTextIcon,
  DocumentArrowUpIcon,
  PencilIcon,
  ArrowUpTrayIcon,
  BoltIcon,
  ChartBarIcon,
  SwatchIcon,
  FolderPlusIcon,
  FolderIcon,
  CodeBracketSquareIcon,
  GlobeAltIcon,
  Squares2X2Icon,
  ArrowDownTrayIcon,
  KeyIcon,
  Square3Stack3DIcon,
  LinkIcon,
  LockClosedIcon,
  SparklesIcon,
  PencilSquareIcon,
  QuestionMarkCircleIcon,
  ChatBubbleLeftIcon,
  ComputerDesktopIcon,
  PaperClipIcon,
  PlayCircleIcon,
  PlayIcon,
  ArrowPathIcon,
  MagnifyingGlassIcon,
  PaperAirplaneIcon,
  Cog6ToothIcon,
  ShieldCheckIcon,
  StopCircleIcon,
  StopIcon,
  QueueListIcon,
  CheckCircleIcon,
  TrashIcon,
  ArrowUturnLeftIcon,
  LockOpenIcon,
  UserCircleIcon,
} from '@heroicons/react/20/solid'

/**
 * Every icon this app uses, backed by Heroicons instead of mx-icons — one
 * file so a call site's `import { SettingsLinear } from '../../../icons'`
 * looks identical to the old `from '../../../icons'` (same names, same {size,
 * color} props), and the ~60-name mapping to real Heroicons exports lives
 * in exactly one place instead of being re-guessed per file.
 *
 * Heroicons components don't take `size`/`color` — they take standard SVG
 * props. This wrapper adapts the old mx-icons call signature onto that,
 * so none of the ~40 call-site files needed to change beyond their import
 * path. `color` relies on the same `currentColor` CSS-inheritance behavior
 * every call site was already passing explicitly.
 */
interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'color'> {
  size?: number
  color?: string
}

function icon(Icon: ComponentType<SVGProps<SVGSVGElement>>) {
  return function WrappedIcon({ size = 20, color, style, ...rest }: IconProps) {
    return createElement(Icon, { width: size, height: size, style: color ? { ...style, color } : style, ...rest })
  }
}

export const AddCircleLinear = icon(PlusCircleIcon)
export const AddLinear = icon(PlusIcon)
export const ArchiveBookLinear = icon(BookOpenIcon)
export const ArchiveLinear = icon(ArchiveBoxIcon)
export const ArrowDownLinear = icon(ChevronDownIcon)
export const ArrowLeftLinear = icon(ArrowLeftIcon)
export const ArrowRightLinear = icon(ArrowRightIcon)
export const ArrowUpLinear = icon(ArrowUpIcon)
export const BookLinear = icon(BookOpenIcon)
export const ChartLinear = icon(ChartBarIcon)
export const CheckLinear = icon(CheckIcon)
export const ColorSwatchLinear = icon(SwatchIcon)
export const ClipboardTextLinear = icon(ClipboardDocumentListIcon)
export const ClockLinear = icon(ClockIcon)
export const CloseCircleLinear = icon(XCircleIcon)
export const CodeLinear = icon(CodeBracketIcon)
export const CommandLinear = icon(CommandLineIcon)
export const CommandSquareLinear = icon(CommandLineIcon)
export const CopyLinear = icon(DocumentDuplicateIcon)
export const CopySuccessLinear = icon(ClipboardDocumentCheckIcon)
export const CpuLinear = icon(CpuChipIcon)
export const DangerLinear = icon(ExclamationTriangleIcon)
export const DocumentLinear = icon(DocumentIcon)
export const DocumentTextLinear = icon(DocumentTextIcon)
export const DocumentUploadLinear = icon(DocumentArrowUpIcon)
export const EditLinear = icon(PencilIcon)
export const ExportLinear = icon(ArrowUpTrayIcon)
export const FlashLinear = icon(BoltIcon)
export const FolderAddLinear = icon(FolderPlusIcon)
export const FolderLinear = icon(FolderIcon)
// Heroicons deliberately ships no brand marks — closest generic stand-in.
export const GithubLinear = icon(CodeBracketSquareIcon)
export const GlobalLinear = icon(GlobeAltIcon)
export const Hierarchy2Linear = icon(Squares2X2Icon)
export const ImportLinear = icon(ArrowDownTrayIcon)
export const KeyLinear = icon(KeyIcon)
export const LayerLinear = icon(Square3Stack3DIcon)
export const Link2Linear = icon(LinkIcon)
export const LockLinear = icon(LockClosedIcon)
export const MagicpenLinear = icon(SparklesIcon)
export const MessageAddLinear = icon(PencilSquareIcon)
export const MessageQuestionLinear = icon(QuestionMarkCircleIcon)
export const MessageSquareLinear = icon(ChatBubbleLeftIcon)
export const MonitorLinear = icon(ComputerDesktopIcon)
export const PaperclipLinear = icon(PaperClipIcon)
export const PlayCircleLinear = icon(PlayCircleIcon)
export const PlayLinear = icon(PlayIcon)
export const RefreshLinear = icon(ArrowPathIcon)
export const SearchNormalLinear = icon(MagnifyingGlassIcon)
export const SendLinear = icon(PaperAirplaneIcon)
export const Setting2Linear = icon(Cog6ToothIcon)
export const SettingLinear = icon(Cog6ToothIcon)
export const SettingsLinear = icon(Cog6ToothIcon)
export const ShieldTickLinear = icon(ShieldCheckIcon)
export const SparkleLinear = icon(SparklesIcon)
export const StopCircleLinear = icon(StopCircleIcon)
export const StopLinear = icon(StopIcon)
export const TaskLinear = icon(QueueListIcon)
export const TickCircleLinear = icon(CheckCircleIcon)
export const TrashLinear = icon(TrashIcon)
export const UndoLinear = icon(ArrowUturnLeftIcon)
export const UnlockLinear = icon(LockOpenIcon)
export const UserOctagonLinear = icon(UserCircleIcon)
