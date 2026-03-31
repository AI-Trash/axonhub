import { useCallback } from 'react';
import { toast } from 'sonner';
import { ZodError } from 'zod';
import * as m from '@/paraglide/messages';

export function useErrorHandler() {
  const handleError = useCallback(
    (error: unknown, context?: string) => {
      let errorMessage = m["common.errors.unknownError"]();

      if (error instanceof ZodError) {
        // Schema validation error
        const fieldErrors =
          error.issues
            ?.map((err: any) => {
              const path = err.path.join('.');
              return `${path}: ${err.message}`;
            })
            .join(', ') || 'Validation failed';

        errorMessage = m["common.errors.validationFailed"]({ details: fieldErrors });

        toast.error(m["common.errors.validationError"](), {
          description: errorMessage,
          duration: 5000,
        });
      } else if (error instanceof Error) {
        errorMessage = error.message;

        if (context) {
          toast.error(m["common.errors.operationFailed"]({ operation: context }), {
            description: errorMessage,
            duration: 4000,
          });
        } else {
          toast.error(errorMessage);
        }
      } else {
        // Unknown error type
        if (context) {
          toast.error(m["common.errors.operationFailed"]({ operation: context }), {
            description: errorMessage,
            duration: 4000,
          });
        } else {
          toast.error(errorMessage);
        }
      }
    },
    []
  );

  return { handleError };
}
