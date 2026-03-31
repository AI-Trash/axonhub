import { useCallback, useRef, useState } from 'react';
import { toast } from 'sonner';
import * as m from '@/paraglide/messages';

type UseCopyToClipboardProps = {
  text: string;
  copyMessage?: string;
};

export function useCopyToClipboard({ text, copyMessage = 'Copied to clipboard!' }: UseCopyToClipboardProps) {
  const [isCopied, setIsCopied] = useState(false);
  const timeoutRef = useRef<NodeJS.Timeout | null>(null);

  const handleCopy = useCallback(() => {
    navigator.clipboard
      .writeText(text)
      .then(() => {
        toast.success(m["common.success.copiedToClipboard"]());
        setIsCopied(true);
        if (timeoutRef.current) {
          clearTimeout(timeoutRef.current);
          timeoutRef.current = null;
        }
        timeoutRef.current = setTimeout(() => {
          setIsCopied(false);
        }, 2000);
      })
      .catch(() => {
        toast.error(m["common.errors.copyFailed"]());
      });
  }, [text, copyMessage, t]);

  return { isCopied, handleCopy };
}
