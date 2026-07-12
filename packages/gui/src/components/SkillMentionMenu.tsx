"use client";

import React from "react";
import { motion } from "motion/react";
import { HugeiconsIcon } from "@hugeicons/react";
import { MagicWand01Icon } from "@hugeicons/core-free-icons";
import { cn } from "@/lib/utils";

export interface MentionableSkill {
  name: string;
  description: string;
}

interface SkillMentionMenuProps {
  skills: MentionableSkill[];
  activeIndex: number;
  onSelect: (skill: MentionableSkill) => void;
  onHover: (index: number) => void;
}

export function SkillMentionMenu({ skills, activeIndex, onSelect, onHover }: SkillMentionMenuProps) {
  if (skills.length === 0) return null;

  return (
    <motion.div
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 6 }}
      transition={{ duration: 0.12, ease: [0.16, 1, 0.3, 1] }}
      className="absolute bottom-full left-0 mb-2 w-full max-w-md rounded-2xl border border-hairline bg-surface-elevated shadow-lg overflow-hidden z-50"
    >
      <div className="px-3.5 py-1.5 text-[10px] font-bold uppercase tracking-wider text-mute font-sans border-b border-hairline">
        Skills
      </div>
      <div className="max-h-[280px] overflow-y-auto p-1.5">
        {skills.map((skill, i) => (
          <button
            key={skill.name}
            onClick={() => onSelect(skill)}
            onMouseEnter={() => onHover(i)}
            className={cn(
              "w-full flex items-center gap-3 px-3 py-2 rounded-xl text-left transition-colors",
              i === activeIndex ? "bg-surface-soft" : "hover:bg-surface-soft/60"
            )}
          >
            <div className="w-6 h-6 rounded-md bg-surface-soft border border-hairline flex items-center justify-center flex-shrink-0 text-ink">
              <HugeiconsIcon icon={MagicWand01Icon} size={13} />
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline gap-2">
                <span className="text-[13px] font-semibold font-mono text-ink flex-shrink-0">@{skill.name}</span>
                <span className="text-[11px] text-body font-sans truncate">{skill.description}</span>
              </div>
            </div>
          </button>
        ))}
      </div>
    </motion.div>
  );
}
