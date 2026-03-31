import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';

import { graphqlRequest } from '@/gql/graphql';
import { USERS_QUERY, CREATE_USER_MUTATION, UPDATE_USER_MUTATION, UPDATE_USER_STATUS_MUTATION } from '@/gql/users';
import { useErrorHandler } from '@/hooks/use-error-handler';

import { User, UserConnection, CreateUserInput, UpdateUserInput, userConnectionSchema, userSchema } from './schema';
import * as m from '@/paraglide/messages';

// Query hooks
export function useUsers(
  variables?: {
    first?: number;
    after?: string;
    orderBy?: { field: 'CREATED_AT'; direction: 'ASC' | 'DESC' };
    where?: Record<string, any>;
  },
  options?: {
    disableAutoFetch?: boolean;
  }
) {
  const { handleError } = useErrorHandler();

  const queryVariables = {
    ...variables,
    orderBy: variables?.orderBy || { field: 'CREATED_AT', direction: 'DESC' },
  };

  return useQuery({
    queryKey: ['users', queryVariables],
    queryFn: async () => {
      try {
        const data = await graphqlRequest<{ users: UserConnection }>(USERS_QUERY, queryVariables);
        return userConnectionSchema.parse(data?.users);
      } catch (error) {
        handleError(error, m["users.messages.loadUsersError"]());
        throw error;
      }
    },
    enabled: !options?.disableAutoFetch,
  });
}

export function useUser(id: string) {
  const { handleError } = useErrorHandler();

  return useQuery({
    queryKey: ['user', id],
    queryFn: async () => {
      try {
        const data = await graphqlRequest<{ users: UserConnection }>(USERS_QUERY, { where: { id } });
        const user = data.users.edges[0]?.node;
        if (!user) {
          throw new Error(m["users.messages.userNotFound"]());
        }
        return userSchema.parse(user);
      } catch (error) {
        handleError(error, m["users.messages.loadUserError"]());
        throw error;
      }
    },
    enabled: !!id,
  });
}

// Mutation hooks
export function useCreateUser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (input: CreateUserInput) => {
      const data = await graphqlRequest<{ createUser: User }>(CREATE_USER_MUTATION, { input });
      return userSchema.parse(data.createUser);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] });
      toast.success(m["users.messages.createSuccess"]());
    },
    onError: (error: any) => {
      toast.error(m["users.messages.createError"]() + `: ${error.message}`);
    },
  });
}

export function useUpdateUser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ id, input }: { id: string; input: UpdateUserInput }) => {
      const data = await graphqlRequest<{ updateUser: User }>(UPDATE_USER_MUTATION, { id, input });
      return userSchema.parse(data.updateUser);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] });
      toast.success(m["users.messages.updateSuccess"]());
    },
    onError: (error: any) => {
      toast.error(m["users.messages.updateError"]() + `: ${error.message}`);
    },
  });
}

export function useUpdateUserStatus() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ id, status }: { id: string; status: 'activated' | 'deactivated' }) => {
      const data = await graphqlRequest<{ updateUserStatus: boolean }>(UPDATE_USER_STATUS_MUTATION, { id, status });
      return data.updateUserStatus;
    },
    onSuccess: (data, variables) => {
      queryClient.invalidateQueries({ queryKey: ['users'] });
      queryClient.invalidateQueries({ queryKey: ['user', variables.id] });
      const statusText = variables.status === 'activated' ? m["users.status.activated"]() : m["users.status.deactivated"]();
      toast.success(m["users.messages.statusUpdateSuccess"]({ status: statusText }));
    },
    onError: (error: any) => {
      toast.error(m["users.messages.statusUpdateError"]() + `: ${error.message}`);
    },
  });
}

export function useDeleteUser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (id: string) => {
      // This is now deprecated, use useUpdateUserStatus instead
      throw new Error('Direct deletion is not supported. Use status update instead.');
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] });
      toast.success(m["users.messages.deleteSuccess"]());
    },
    onError: (error: any) => {
      toast.error(m["users.messages.deleteError"]() + `: ${error.message}`);
    },
  });
}

// Export users for compatibility
export const users = {
  useUsers,
  useUser,
  useCreateUser,
  useUpdateUser,
  useDeleteUser,
};
